use crate::hint_processor::builtin_hint_processor::hint_utils::{
    get_integer_from_var_name, get_ptr_from_var_name, get_relocatable_from_var_name,
};
use crate::hint_processor::hint_processor_definition::HintReference;
use crate::hint_processor::proxies::exec_scopes_proxy::ExecutionScopesProxy;
use crate::hint_processor::proxies::vm_proxy::VMProxy;
use crate::{
    bigint,
    serde::deserialize_program::ApTracking,
    types::{relocatable::MaybeRelocatable, relocatable::Relocatable},
    vm::errors::vm_errors::VirtualMachineError,
};
use num_bigint::{BigInt, Sign};
use num_traits::Signed;
use num_traits::ToPrimitive;
use sha3::{Digest, Keccak256};
use std::{cmp, collections::HashMap, ops::Shl};

/* Implements hint:
   %{
       from eth_hash.auto import keccak

       data, length = ids.data, ids.length

       if '__keccak_max_size' in globals():
           assert length <= __keccak_max_size, \
               f'unsafe_keccak() can only be used with length<={__keccak_max_size}. ' \
               f'Got: length={length}.'

       keccak_input = bytearray()
       for word_i, byte_i in enumerate(range(0, length, 16)):
           word = memory[data + word_i]
           n_bytes = min(16, length - byte_i)
           assert 0 <= word < 2 ** (8 * n_bytes)
           keccak_input += word.to_bytes(n_bytes, 'big')

       hashed = keccak(keccak_input)
       ids.high = int.from_bytes(hashed[:16], 'big')
       ids.low = int.from_bytes(hashed[16:32], 'big')
   %}
*/
pub fn unsafe_keccak(
    vm_proxy: &mut VMProxy,
    exec_scopes_proxy: &mut ExecutionScopesProxy,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<(), VirtualMachineError> {
    let length = get_integer_from_var_name("length", vm_proxy, ids_data, ap_tracking)?;

    if let Ok(keccak_max_size) = exec_scopes_proxy.get_int("__keccak_max_size") {
        if length > &keccak_max_size {
            return Err(VirtualMachineError::KeccakMaxSize(
                length.clone(),
                keccak_max_size,
            ));
        }
    }

    // `data` is an array, represented by a pointer to the first element.
    let data = get_ptr_from_var_name("data", vm_proxy, ids_data, ap_tracking)?;

    let high_addr = get_relocatable_from_var_name("high", vm_proxy, ids_data, ap_tracking)?;
    let low_addr = get_relocatable_from_var_name("low", vm_proxy, ids_data, ap_tracking)?;

    // transform to u64 to make ranges cleaner in the for loop below
    let u64_length = length
        .to_u64()
        .ok_or_else(|| VirtualMachineError::InvalidKeccakInputLength(length.clone()))?;

    let mut keccak_input = Vec::new();
    for (word_i, byte_i) in (0..u64_length).step_by(16).enumerate() {
        let word_addr = Relocatable {
            segment_index: data.segment_index,
            offset: data.offset + word_i,
        };

        let word = vm_proxy.memory.get_integer(&word_addr)?;
        let n_bytes = cmp::min(16, u64_length - byte_i);

        if word.is_negative() || word >= &bigint!(1).shl(8 * (n_bytes as u32)) {
            return Err(VirtualMachineError::InvalidWordSize(word.clone()));
        }

        let (_, mut bytes) = word.to_bytes_be();
        let mut bytes = {
            let n_word_bytes = &bytes.len();
            left_pad(&mut bytes, (n_bytes as usize - n_word_bytes) as usize)
        };

        keccak_input.append(&mut bytes);
    }

    let mut hasher = Keccak256::new();
    hasher.update(keccak_input);

    let hashed = hasher.finalize();

    let high = BigInt::from_bytes_be(Sign::Plus, &hashed[..16]);
    let low = BigInt::from_bytes_be(Sign::Plus, &hashed[16..32]);

    vm_proxy.memory.insert_value(&high_addr, &high)?;
    vm_proxy.memory.insert_value(&low_addr, &low)
}

/*
Implements hint:

    %{
        from eth_hash.auto import keccak
        keccak_input = bytearray()
        n_elms = ids.keccak_state.end_ptr - ids.keccak_state.start_ptr
        for word in memory.get_range(ids.keccak_state.start_ptr, n_elms):
            keccak_input += word.to_bytes(16, 'big')
        hashed = keccak(keccak_input)
        ids.high = int.from_bytes(hashed[:16], 'big')
        ids.low = int.from_bytes(hashed[16:32], 'big')
    %}

 */
pub fn unsafe_keccak_finalize(
    vm_proxy: &mut VMProxy,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<(), VirtualMachineError> {
    /* -----------------------------
    Just for reference (cairo code):
    struct KeccakState:
        member start_ptr : felt*
        member end_ptr : felt*
    end
    ----------------------------- */

    let keccak_state_ptr =
        get_relocatable_from_var_name("keccak_state", vm_proxy, ids_data, ap_tracking)?;

    // as `keccak_state` is a struct, the pointer to the struct is the same as the pointer to the first element.
    // this is why to get the pointer stored in the field `start_ptr` it is enough to pass the variable name as
    // `keccak_state`, which is the one that appears in the reference manager of the compiled JSON.
    let start_ptr = get_ptr_from_var_name("keccak_state", vm_proxy, ids_data, ap_tracking)?;

    // in the KeccakState struct, the field `end_ptr` is the second one, so this variable should be get from
    // the memory cell contiguous to the one where KeccakState is pointing to.
    let end_ptr = vm_proxy.memory.get_relocatable(&Relocatable {
        segment_index: keccak_state_ptr.segment_index,
        offset: keccak_state_ptr.offset + 1,
    })?;

    // this is not very nice code, we should consider adding the sub() method for Relocatable's
    let maybe_rel_start_ptr = MaybeRelocatable::RelocatableValue(start_ptr);
    let maybe_rel_end_ptr = MaybeRelocatable::RelocatableValue(end_ptr.clone());

    let n_elems = maybe_rel_end_ptr
        .sub(&maybe_rel_start_ptr, vm_proxy.prime)?
        .get_int_ref()?
        .to_usize()
        .ok_or(VirtualMachineError::BigintToUsizeFail)?;

    let mut keccak_input = Vec::new();
    let range = vm_proxy
        .memory
        .get_range(&maybe_rel_start_ptr, n_elems)
        .map_err(VirtualMachineError::MemoryError)?;

    check_no_nones_in_range(&range)?;

    for maybe_reloc_word in range.iter() {
        let word = maybe_reloc_word
            .ok_or(VirtualMachineError::ExpectedIntAtRange(
                maybe_reloc_word.cloned(),
            ))?
            .get_int_ref()?;

        let (_, mut bytes) = word.to_bytes_be();
        let mut bytes = {
            let n_word_bytes = &bytes.len();
            left_pad(&mut bytes, 16 - n_word_bytes)
        };
        keccak_input.append(&mut bytes);
    }

    let mut hasher = Keccak256::new();
    hasher.update(keccak_input);

    let hashed = hasher.finalize();

    let high_addr = get_relocatable_from_var_name("high", vm_proxy, ids_data, ap_tracking)?;
    let low_addr = get_relocatable_from_var_name("low", vm_proxy, ids_data, ap_tracking)?;

    let high = BigInt::from_bytes_be(Sign::Plus, &hashed[..16]);
    let low = BigInt::from_bytes_be(Sign::Plus, &hashed[16..32]);

    vm_proxy.memory.insert_value(&high_addr, &high)?;
    vm_proxy.memory.insert_value(&low_addr, &low)
}

fn left_pad(bytes_vector: &mut [u8], n_zeros: usize) -> Vec<u8> {
    let mut res: Vec<u8> = vec![0; n_zeros];
    res.extend(bytes_vector.iter());

    res
}

fn check_no_nones_in_range<T>(range: &Vec<Option<T>>) -> Result<(), VirtualMachineError> {
    for memory_cell in range {
        memory_cell
            .as_ref()
            .ok_or(VirtualMachineError::NoneInMemoryRange)?;
    }

    Ok(())
}

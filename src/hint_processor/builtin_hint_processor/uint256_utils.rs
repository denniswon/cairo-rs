use crate::bigint;
use crate::hint_processor::builtin_hint_processor::hint_utils::{
    get_integer_from_var_name, get_relocatable_from_var_name,
};
use crate::hint_processor::proxies::vm_proxy::VMProxy;
use crate::math_utils::isqrt;
use crate::serde::deserialize_program::ApTracking;
use crate::vm::errors::vm_errors::VirtualMachineError;
use num_bigint::BigInt;
use num_integer::{div_rem, Integer};
use num_traits::Signed;
use std::collections::HashMap;
use std::ops::{Shl, Shr};

use crate::hint_processor::builtin_hint_processor::hint_utils::{
    insert_value_from_var_name, insert_value_into_ap,
};
use crate::hint_processor::hint_processor_definition::HintReference;

/*
Implements hint:
%{
    sum_low = ids.a.low + ids.b.low
    ids.carry_low = 1 if sum_low >= ids.SHIFT else 0
    sum_high = ids.a.high + ids.b.high + ids.carry_low
    ids.carry_high = 1 if sum_high >= ids.SHIFT else 0
%}
*/
pub fn uint256_add(
    vm_proxy: &mut VMProxy,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<(), VirtualMachineError> {
    let shift: BigInt = bigint!(2).pow(128);

    let a_relocatable = get_relocatable_from_var_name("a", vm_proxy, ids_data, ap_tracking)?;
    let b_relocatable = get_relocatable_from_var_name("b", vm_proxy, ids_data, ap_tracking)?;
    let a_low = vm_proxy.memory.get_integer(&a_relocatable)?;
    let a_high = vm_proxy.memory.get_integer(&(a_relocatable + 1))?;
    let b_low = vm_proxy.memory.get_integer(&b_relocatable)?;
    let b_high = vm_proxy.memory.get_integer(&(b_relocatable + 1))?;

    //Main logic
    //sum_low = ids.a.low + ids.b.low
    //ids.carry_low = 1 if sum_low >= ids.SHIFT else 0
    //sum_high = ids.a.high + ids.b.high + ids.carry_low
    //ids.carry_high = 1 if sum_high >= ids.SHIFT else 0

    let carry_low = if a_low + b_low >= shift {
        bigint!(1)
    } else {
        bigint!(0)
    };

    let carry_high = if a_high + b_high + &carry_low >= shift {
        bigint!(1)
    } else {
        bigint!(0)
    };
    insert_value_from_var_name("carry_high", carry_high, vm_proxy, ids_data, ap_tracking)?;
    insert_value_from_var_name("carry_low", carry_low, vm_proxy, ids_data, ap_tracking)
}

/*
Implements hint:
%{
    ids.low = ids.a & ((1<<64) - 1)
    ids.high = ids.a >> 64
%}
*/
pub fn split_64(
    vm_proxy: &mut VMProxy,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<(), VirtualMachineError> {
    let a = get_integer_from_var_name("a", vm_proxy, ids_data, ap_tracking)?;
    let mut digits = a.iter_u64_digits();
    let low = bigint!(digits.next().unwrap_or(0u64));
    let high = if digits.len() <= 1 {
        bigint!(digits.next().unwrap_or(0u64))
    } else {
        a.shr(64_usize)
    };
    insert_value_from_var_name("high", bigint!(high), vm_proxy, ids_data, ap_tracking)?;
    insert_value_from_var_name("low", bigint!(low), vm_proxy, ids_data, ap_tracking)
}

/*
Implements hint:
%{
    from starkware.python.math_utils import isqrt
    n = (ids.n.high << 128) + ids.n.low
    root = isqrt(n)
    assert 0 <= root < 2 ** 128
    ids.root.low = root
    ids.root.high = 0
%}
*/
pub fn uint256_sqrt(
    vm_proxy: &mut VMProxy,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<(), VirtualMachineError> {
    let n_addr = get_relocatable_from_var_name("n", vm_proxy, ids_data, ap_tracking)?;
    let root_addr = get_relocatable_from_var_name("root", vm_proxy, ids_data, ap_tracking)?;
    let n_low = vm_proxy.memory.get_integer(&n_addr)?;
    let n_high = vm_proxy.memory.get_integer(&(n_addr + 1))?;

    //Main logic
    //from starkware.python.math_utils import isqrt
    //n = (ids.n.high << 128) + ids.n.low
    //root = isqrt(n)
    //assert 0 <= root < 2 ** 128
    //ids.root.low = root
    //ids.root.high = 0

    let root = isqrt(&(n_high.shl(128_usize) + n_low))?;

    if root.is_negative() || root >= bigint!(1).shl(128) {
        return Err(VirtualMachineError::AssertionFailed(format!(
            "assert 0 <= {} < 2 ** 128",
            &root
        )));
    }
    vm_proxy.memory.insert_value(&root_addr, root)?;
    vm_proxy.memory.insert_value(&(root_addr + 1), bigint!(0))
}

/*
Implements hint:
%{ memory[ap] = 1 if 0 <= (ids.a.high % PRIME) < 2 ** 127 else 0 %}
*/
pub fn uint256_signed_nn(
    vm_proxy: &mut VMProxy,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<(), VirtualMachineError> {
    let a_addr = get_relocatable_from_var_name("a", vm_proxy, ids_data, ap_tracking)?;
    let a_high = vm_proxy.memory.get_integer(&(a_addr + 1))?;
    //Main logic
    //memory[ap] = 1 if 0 <= (ids.a.high % PRIME) < 2 ** 127 else 0
    let result: BigInt =
        if !a_high.is_negative() && (a_high.mod_floor(vm_proxy.prime)) <= bigint!(i128::MAX) {
            bigint!(1)
        } else {
            bigint!(0)
        };
    insert_value_into_ap(&mut vm_proxy.memory, vm_proxy.run_context, result)
}

/*
Implements hint:
%{
    a = (ids.a.high << 128) + ids.a.low
    div = (ids.div.high << 128) + ids.div.low
    quotient, remainder = divmod(a, div)

    ids.quotient.low = quotient & ((1 << 128) - 1)
    ids.quotient.high = quotient >> 128
    ids.remainder.low = remainder & ((1 << 128) - 1)
    ids.remainder.high = remainder >> 128
%}
*/
pub fn uint256_unsigned_div_rem(
    vm_proxy: &mut VMProxy,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<(), VirtualMachineError> {
    let a_addr = get_relocatable_from_var_name("a", vm_proxy, ids_data, ap_tracking)?;
    let div_addr = get_relocatable_from_var_name("div", vm_proxy, ids_data, ap_tracking)?;
    let quotient_addr = get_relocatable_from_var_name("quotient", vm_proxy, ids_data, ap_tracking)?;
    let remainder_addr =
        get_relocatable_from_var_name("remainder", vm_proxy, ids_data, ap_tracking)?;

    let a_low = vm_proxy.memory.get_integer(&a_addr)?;
    let a_high = vm_proxy.memory.get_integer(&(a_addr + 1))?;
    let div_low = vm_proxy.memory.get_integer(&div_addr)?;
    let div_high = vm_proxy.memory.get_integer(&(div_addr + 1))?;

    //Main logic
    //a = (ids.a.high << 128) + ids.a.low
    //div = (ids.div.high << 128) + ids.div.low
    //quotient, remainder = divmod(a, div)

    //ids.quotient.low = quotient & ((1 << 128) - 1)
    //ids.quotient.high = quotient >> 128
    //ids.remainder.low = remainder & ((1 << 128) - 1)
    //ids.remainder.high = remainder >> 128

    let a = a_high.shl(128_usize) + a_low;
    let div = div_high.shl(128_usize) + div_low;
    //a and div will always be positive numbers
    //Then, Rust div_rem equals Python divmod
    let (quotient, remainder) = div_rem(a, div);

    let quotient_low = &quotient & bigint!(u128::MAX);
    let quotient_high = quotient.shr(128_usize);

    let remainder_low = &remainder & bigint!(u128::MAX);
    let remainder_high = remainder.shr(128_usize);

    //Insert ids.quotient.low
    vm_proxy.memory.insert_value(&quotient_addr, quotient_low)?;
    //Insert ids.quotient.high
    vm_proxy
        .memory
        .insert_value(&(quotient_addr + 1), quotient_high)?;
    //Insert ids.remainder.low
    vm_proxy
        .memory
        .insert_value(&remainder_addr, remainder_low)?;
    //Insert ids.remainder.high
    vm_proxy
        .memory
        .insert_value(&(remainder_addr + 1), remainder_high)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::any_box;
    use crate::bigint_str;
    use crate::hint_processor::builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor;
    use crate::hint_processor::builtin_hint_processor::builtin_hint_processor_definition::HintProcessorData;
    use crate::hint_processor::hint_processor_definition::HintProcessor;
    use crate::hint_processor::proxies::exec_scopes_proxy::get_exec_scopes_proxy;
    use crate::hint_processor::proxies::vm_proxy::get_vm_proxy;
    use crate::types::exec_scope::ExecutionScopes;
    use crate::types::relocatable::MaybeRelocatable;
    use crate::utils::test_utils::*;
    use crate::vm::errors::memory_errors::MemoryError;
    use crate::vm::vm_core::VirtualMachine;
    use crate::vm::vm_memory::memory::Memory;
    use crate::{bigint, vm::runners::builtin_runner::RangeCheckBuiltinRunner};
    use num_bigint::{BigInt, Sign};
    use std::any::Any;

    #[test]
    fn run_uint256_add_ok() {
        let hint_code = "sum_low = ids.a.low + ids.b.low\nids.carry_low = 1 if sum_low >= ids.SHIFT else 0\nsum_high = ids.a.high + ids.b.high + ids.carry_low\nids.carry_high = 1 if sum_high >= ids.SHIFT else 0";
        let mut vm = vm_with_range_check!();
        //Initialize fp
        vm.run_context.fp = 10;
        //Create hint_data
        let ids_data = HashMap::from([
            ("a".to_string(), HintReference::new_simple(-6)),
            ("b".to_string(), HintReference::new_simple(-4)),
            ("carry_high".to_string(), HintReference::new_simple(3)),
            ("carry_low".to_string(), HintReference::new_simple(2)),
        ]);
        let hint_data = HintProcessorData::new_default(hint_code.to_string(), ids_data);
        vm.memory = memory![((1, 4), 2), ((1, 5), 3), ((1, 6), 4)];
        vm.memory
            .insert(
                &MaybeRelocatable::from((1, 7)),
                &MaybeRelocatable::from(bigint!(2).pow(128)),
            )
            .unwrap();

        let vm_proxy = &mut get_vm_proxy(&mut vm);
        //Execute the hint
        let hint_processor = BuiltinHintProcessor::new_empty();
        assert_eq!(
            hint_processor.execute_hint(vm_proxy, exec_scopes_proxy_ref!(), &any_box!(hint_data)),
            Ok(())
        );

        //Check hint memory inserts
        check_memory![&vm.memory, ((1, 12), 0), ((1, 13), 1)];
    }

    #[test]
    fn run_uint256_add_fail_inserts() {
        let hint_code = "sum_low = ids.a.low + ids.b.low\nids.carry_low = 1 if sum_low >= ids.SHIFT else 0\nsum_high = ids.a.high + ids.b.high + ids.carry_low\nids.carry_high = 1 if sum_high >= ids.SHIFT else 0";
        let mut vm = vm_with_range_check!();
        for _ in 0..3 {
            vm.segments.add(&mut vm.memory, None);
        }

        //Initialize fp
        vm.run_context.fp = 10;

        //Create hint_data
        let ids_data = HashMap::from([
            ("a".to_string(), HintReference::new_simple(-6)),
            ("b".to_string(), HintReference::new_simple(-4)),
            ("carry_high".to_string(), HintReference::new_simple(3)),
            ("carry_low".to_string(), HintReference::new_simple(2)),
        ]);
        let hint_data = HintProcessorData::new_default(hint_code.to_string(), ids_data);

        //Insert ids.a.low into memory
        vm.memory
            .insert(
                &MaybeRelocatable::from((1, 4)),
                &MaybeRelocatable::from(bigint!(2)),
            )
            .unwrap();
        //Insert ids.a.high into memory
        vm.memory
            .insert(
                &MaybeRelocatable::from((1, 5)),
                &MaybeRelocatable::from(bigint!(3)),
            )
            .unwrap();
        //Insert ids.b.low into memory
        vm.memory
            .insert(
                &MaybeRelocatable::from((1, 6)),
                &MaybeRelocatable::from(bigint!(4)),
            )
            .unwrap();
        //Insert ids.b.high into memory
        vm.memory
            .insert(
                &MaybeRelocatable::from((1, 7)),
                &MaybeRelocatable::from(bigint!(2)),
            )
            .unwrap();

        //Insert a value in the ids.carry_low address, so the hint insertion fails
        vm.memory
            .insert(
                &MaybeRelocatable::from((1, 12)),
                &MaybeRelocatable::from(bigint!(2)),
            )
            .unwrap();

        let vm_proxy = &mut get_vm_proxy(&mut vm);
        //Execute the hint
        let hint_processor = BuiltinHintProcessor::new_empty();
        assert_eq!(
            hint_processor.execute_hint(vm_proxy, exec_scopes_proxy_ref!(), &any_box!(hint_data)),
            Err(VirtualMachineError::MemoryError(
                MemoryError::InconsistentMemory(
                    MaybeRelocatable::from((1, 12)),
                    MaybeRelocatable::from(bigint!(2)),
                    MaybeRelocatable::from(bigint!(0))
                )
            ))
        );
    }

    #[test]
    fn run_split_64_ok() {
        let hint_code = "ids.low = ids.a & ((1<<64) - 1)\nids.high = ids.a >> 64";
        let mut vm = vm_with_range_check!();
        for _ in 0..3 {
            vm.segments.add(&mut vm.memory, None);
        }

        //Initialize fp
        vm.run_context.fp = 10;

        //Create hint_data
        let ids_data = HashMap::from([
            ("a".to_string(), HintReference::new_simple(-3)),
            ("high".to_string(), HintReference::new_simple(1)),
            ("low".to_string(), HintReference::new_simple(0)),
        ]);
        let hint_data = HintProcessorData::new_default(hint_code.to_string(), ids_data);

        //Insert ids.a into memory
        vm.memory
            .insert(
                &MaybeRelocatable::from((1, 7)),
                &MaybeRelocatable::from(bigint_str!(b"850981239023189021389081239089023")),
            )
            .unwrap();

        let vm_proxy = &mut get_vm_proxy(&mut vm);
        //Execute the hint
        let hint_processor = BuiltinHintProcessor::new_empty();
        assert_eq!(
            hint_processor.execute_hint(vm_proxy, exec_scopes_proxy_ref!(), &any_box!(hint_data)),
            Ok(())
        );

        //Check hint memory inserts
        //ids.low, ids.high
        check_memory![
            &vm.memory,
            ((1, 10), 7249717543555297151_u64),
            ((1, 11), 46131785404667_u64)
        ];
    }

    #[test]
    fn run_split_64_with_big_a() {
        let hint_code = "ids.low = ids.a & ((1<<64) - 1)\nids.high = ids.a >> 64";
        let mut vm = vm_with_range_check!();
        for _ in 0..3 {
            vm.segments.add(&mut vm.memory, None);
        }

        //Initialize fp
        vm.run_context.fp = 10;

        //Create ids_data
        let ids_data = HashMap::from([
            ("a".to_string(), HintReference::new_simple(-3)),
            ("high".to_string(), HintReference::new_simple(1)),
            ("low".to_string(), HintReference::new_simple(0)),
        ]);
        let hint_data = HintProcessorData::new_default(hint_code.to_string(), ids_data);
        //Insert ids.a into memory
        vm.memory
            .insert(
                &MaybeRelocatable::from((1, 7)),
                &MaybeRelocatable::from(bigint_str!(b"400066369019890261321163226850167045262")),
            )
            .unwrap();

        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let hint_processor = BuiltinHintProcessor::new_empty();
        assert_eq!(
            hint_processor.execute_hint(vm_proxy, exec_scopes_proxy_ref!(), &any_box!(hint_data)),
            Ok(())
        );

        //Check hint memory inserts
        //ids.low, ids.high
        check_memory![
            &vm.memory,
            ((1, 10), 2279400676465785998_u64),
            ((1, 11), 21687641321487626429_u128)
        ];
    }

    #[test]
    fn run_split_64_memory_error() {
        let hint_code = "ids.low = ids.a & ((1<<64) - 1)\nids.high = ids.a >> 64";
        let mut vm = vm_with_range_check!();
        for _ in 0..3 {
            vm.segments.add(&mut vm.memory, None);
        }

        //Initialize fp
        vm.run_context.fp = 10;

        //Create hint_data
        let ids_data = HashMap::from([
            ("a".to_string(), HintReference::new_simple(-3)),
            ("high".to_string(), HintReference::new_simple(1)),
            ("low".to_string(), HintReference::new_simple(0)),
        ]);
        let hint_data = HintProcessorData::new_default(hint_code.to_string(), ids_data);

        //Insert ids.a into memory
        vm.memory
            .insert(
                &MaybeRelocatable::from((1, 7)),
                &MaybeRelocatable::from(bigint_str!(b"850981239023189021389081239089023")),
            )
            .unwrap();

        //Insert a value in the ids.low address, so the hint insert fails
        vm.memory
            .insert(
                &MaybeRelocatable::from((1, 10)),
                &MaybeRelocatable::from(bigint!(0)),
            )
            .unwrap();

        let vm_proxy = &mut get_vm_proxy(&mut vm);
        //Execute the hint
        let hint_processor = BuiltinHintProcessor::new_empty();
        assert_eq!(
            hint_processor.execute_hint(vm_proxy, exec_scopes_proxy_ref!(), &any_box!(hint_data)),
            Err(VirtualMachineError::MemoryError(
                MemoryError::InconsistentMemory(
                    MaybeRelocatable::from((1, 10)),
                    MaybeRelocatable::from(bigint!(0)),
                    MaybeRelocatable::from(bigint_str!(b"7249717543555297151"))
                )
            ))
        );
    }

    #[test]
    fn run_uint256_sqrt_ok() {
        let hint_code = "from starkware.python.math_utils import isqrt\nn = (ids.n.high << 128) + ids.n.low\nroot = isqrt(n)\nassert 0 <= root < 2 ** 128\nids.root.low = root\nids.root.high = 0";
        let mut vm = vm_with_range_check!();
        for _ in 0..3 {
            vm.segments.add(&mut vm.memory, None);
        }

        //Initialize fp
        vm.run_context.fp = 5;
        //Create hint_data
        let ids_data = HashMap::from([
            ("n".to_string(), HintReference::new_simple(-5)),
            ("root".to_string(), HintReference::new_simple(0)),
        ]);
        let hint_data = HintProcessorData::new_default(hint_code.to_string(), ids_data);
        vm.memory = memory![((1, 0), 17), ((1, 1), 7)];
        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let hint_processor = BuiltinHintProcessor::new_empty();
        assert_eq!(
            hint_processor.execute_hint(vm_proxy, exec_scopes_proxy_ref!(), &any_box!(hint_data)),
            Ok(())
        );

        //Check hint memory inserts
        //ids.root.low, ids.root.high
        check_memory![&vm.memory, ((1, 5), 48805497317890012913_u128), ((1, 6), 0)];
    }

    #[test]
    fn run_uint256_sqrt_assert_error() {
        let hint_code = "from starkware.python.math_utils import isqrt\nn = (ids.n.high << 128) + ids.n.low\nroot = isqrt(n)\nassert 0 <= root < 2 ** 128\nids.root.low = root\nids.root.high = 0";
        let mut vm = vm_with_range_check!();
        for _ in 0..3 {
            vm.segments.add(&mut vm.memory, None);
        }

        //Initialize fp
        vm.run_context.fp = 5;

        //Create hint_data
        let ids_data = HashMap::from([
            ("n".to_string(), HintReference::new_simple(-5)),
            ("root".to_string(), HintReference::new_simple(0)),
        ]);
        let hint_data = HintProcessorData::new_default(hint_code.to_string(), ids_data);
        vm.memory = memory![((1, 0), 0)];
        //Insert ids.n.high into memory
        vm.memory
            .insert(
                &MaybeRelocatable::from((1, 1)),
                &MaybeRelocatable::from(bigint_str!(b"340282366920938463463374607431768211458")),
            )
            .unwrap();
        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let hint_processor = BuiltinHintProcessor::new_empty();
        assert_eq!(
            hint_processor.execute_hint(vm_proxy, exec_scopes_proxy_ref!(), &any_box!(hint_data)),
            Err(VirtualMachineError::AssertionFailed(String::from(
                "assert 0 <= 340282366920938463463374607431768211456 < 2 ** 128"
            )))
        );
    }

    #[test]
    fn run_uint256_invalid_memory_insert() {
        let hint_code = "from starkware.python.math_utils import isqrt\nn = (ids.n.high << 128) + ids.n.low\nroot = isqrt(n)\nassert 0 <= root < 2 ** 128\nids.root.low = root\nids.root.high = 0";
        let mut vm = vm_with_range_check!();
        for _ in 0..3 {
            vm.segments.add(&mut vm.memory, None);
        }

        //Initialize fp
        vm.run_context.fp = 5;

        //Create hint_data
        let ids_data = HashMap::from([
            ("n".to_string(), HintReference::new_simple(-5)),
            ("root".to_string(), HintReference::new_simple(0)),
        ]);
        let hint_data = HintProcessorData::new_default(hint_code.to_string(), ids_data);

        //Insert  ids.n.low into memory
        vm.memory = memory![((1, 0), 17), ((1, 1), 7), ((1, 5), 1)];
        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let hint_processor = BuiltinHintProcessor::new_empty();
        assert_eq!(
            hint_processor.execute_hint(vm_proxy, exec_scopes_proxy_ref!(), &any_box!(hint_data)),
            Err(VirtualMachineError::MemoryError(
                MemoryError::InconsistentMemory(
                    MaybeRelocatable::from((1, 5)),
                    MaybeRelocatable::from(bigint!(1)),
                    MaybeRelocatable::from(bigint_str!(b"48805497317890012913")),
                )
            ))
        );
    }

    #[test]
    fn run_signed_nn_ok_result_one() {
        let hint_code = "memory[ap] = 1 if 0 <= (ids.a.high % PRIME) < 2 ** 127 else 0";
        let mut vm = vm_with_range_check!();
        for _ in 0..3 {
            vm.segments.add(&mut vm.memory, None);
        }

        //Initialize fp
        vm.run_context.fp = 4;
        vm.run_context.ap = 5;
        //Create hint_data
        let ids_data = HashMap::from([("a".to_string(), HintReference::new_simple(-4))]);
        let hint_data = HintProcessorData::new_default(hint_code.to_string(), ids_data);
        //Insert ids.a.high into memory
        vm.memory
            .insert(
                &MaybeRelocatable::from((1, 1)),
                &MaybeRelocatable::from(&vm.prime + i128::MAX),
            )
            .unwrap();
        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let hint_processor = BuiltinHintProcessor::new_empty();
        assert_eq!(
            hint_processor.execute_hint(vm_proxy, exec_scopes_proxy_ref!(), &any_box!(hint_data)),
            Ok(())
        );

        //Check hint memory insert
        //memory[ap] = 1 if 0 <= (ids.a.high % PRIME) < 2 ** 127 else 0
        check_memory![&vm.memory, ((1, 5), 1)];
    }

    #[test]
    fn run_signed_nn_ok_result_zero() {
        let hint_code = "memory[ap] = 1 if 0 <= (ids.a.high % PRIME) < 2 ** 127 else 0";
        let mut vm = vm_with_range_check!();
        for _ in 0..3 {
            vm.segments.add(&mut vm.memory, None);
        }

        //Initialize fp
        vm.run_context.fp = 4;
        vm.run_context.ap = 5;

        //Create hint_data
        let ids_data = HashMap::from([("a".to_string(), HintReference::new_simple(-4))]);
        let hint_data = HintProcessorData::new_default(hint_code.to_string(), ids_data);

        //Insert ids.a.high into memory
        vm.memory
            .insert(
                &MaybeRelocatable::from((1, 1)),
                &MaybeRelocatable::from(bigint!(1).shl(127_i64) + &vm.prime),
            )
            .unwrap();

        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let hint_processor = BuiltinHintProcessor::new_empty();
        assert_eq!(
            hint_processor.execute_hint(vm_proxy, exec_scopes_proxy_ref!(), &any_box!(hint_data)),
            Ok(())
        );

        //Check hint memory insert
        //memory[ap] = 1 if 0 <= (ids.a.high % PRIME) < 2 ** 127 else 0
        check_memory![&vm.memory, ((1, 5), 0)];
    }

    #[test]
    fn run_signed_nn_ok_invalid_memory_insert() {
        let hint_code = "memory[ap] = 1 if 0 <= (ids.a.high % PRIME) < 2 ** 127 else 0";
        let mut vm = vm_with_range_check!();
        for _ in 0..3 {
            vm.segments.add(&mut vm.memory, None);
        }

        //Initialize fp
        vm.run_context.fp = 4;
        vm.run_context.ap = 5;

        //Create hint_data
        let ids_data = HashMap::from([("a".to_string(), HintReference::new_simple(-4))]);
        let hint_data = HintProcessorData::new_default(hint_code.to_string(), ids_data);

        //Insert ids.a.high into memory
        vm.memory
            .insert(
                &MaybeRelocatable::from((1, 1)),
                &MaybeRelocatable::from(bigint!(1)),
            )
            .unwrap();

        //Insert a value in ap so the hint insert fails
        vm.memory
            .insert(
                &mut vm.run_context.get_ap(),
                &MaybeRelocatable::from(bigint!(55)),
            )
            .unwrap();
        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let hint_processor = BuiltinHintProcessor::new_empty();
        assert_eq!(
            hint_processor.execute_hint(vm_proxy, exec_scopes_proxy_ref!(), &any_box!(hint_data)),
            Err(VirtualMachineError::MemoryError(
                MemoryError::InconsistentMemory(
                    MaybeRelocatable::from((1, 5)),
                    MaybeRelocatable::from(bigint!(55)),
                    MaybeRelocatable::from(bigint!(1)),
                )
            ))
        );
    }

    #[test]
    fn run_unsigned_div_rem_ok() {
        let hint_code = "a = (ids.a.high << 128) + ids.a.low\ndiv = (ids.div.high << 128) + ids.div.low\nquotient, remainder = divmod(a, div)\n\nids.quotient.low = quotient & ((1 << 128) - 1)\nids.quotient.high = quotient >> 128\nids.remainder.low = remainder & ((1 << 128) - 1)\nids.remainder.high = remainder >> 128";
        let mut vm = vm_with_range_check!();
        //Initialize fp
        vm.run_context.fp = 10;
        //Create hint_data
        let ids_data = HashMap::from([
            ("a".to_string(), HintReference::new_simple(-6)),
            ("div".to_string(), HintReference::new_simple(-4)),
            ("quotient".to_string(), HintReference::new_simple(0)),
            ("remainder".to_string(), HintReference::new_simple(2)),
        ]);
        let hint_data = HintProcessorData::new_default(hint_code.to_string(), ids_data);
        //Insert ids into memory
        vm.memory = memory![((1, 4), 89), ((1, 5), 72), ((1, 6), 3), ((1, 7), 7)];
        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let hint_processor = BuiltinHintProcessor::new_empty();
        assert_eq!(
            hint_processor.execute_hint(vm_proxy, exec_scopes_proxy_ref!(), &any_box!(hint_data)),
            Ok(())
        );

        //Check hint memory inserts
        //ids.quotient.low, ids.quotient.high, ids.remainder.low, ids.remainder.high

        check_memory![
            &vm.memory,
            ((1, 10), 10),
            ((1, 11), 0),
            ((1, 12), 59),
            ((1, 13), 2)
        ];
    }

    #[test]
    fn run_unsigned_div_rem_invalid_memory_insert() {
        let hint_code = "a = (ids.a.high << 128) + ids.a.low\ndiv = (ids.div.high << 128) + ids.div.low\nquotient, remainder = divmod(a, div)\n\nids.quotient.low = quotient & ((1 << 128) - 1)\nids.quotient.high = quotient >> 128\nids.remainder.low = remainder & ((1 << 128) - 1)\nids.remainder.high = remainder >> 128";
        let mut vm = vm_with_range_check!();
        //Initialize fp
        vm.run_context.fp = 10;

        //Create hint_data
        let ids_data = HashMap::from([
            ("a".to_string(), HintReference::new_simple(-6)),
            ("div".to_string(), HintReference::new_simple(-4)),
            ("quotient".to_string(), HintReference::new_simple(0)),
            ("remainder".to_string(), HintReference::new_simple(2)),
        ]);
        let hint_data = HintProcessorData::new_default(hint_code.to_string(), ids_data);
        //Insert ids into memory
        vm.memory = memory![
            ((1, 4), 89),
            ((1, 5), 72),
            ((1, 6), 3),
            ((1, 7), 7),
            ((1, 10), 0)
        ];
        //Execute the hint
        let vm_proxy = &mut get_vm_proxy(&mut vm);
        let hint_processor = BuiltinHintProcessor::new_empty();
        assert_eq!(
            hint_processor.execute_hint(vm_proxy, exec_scopes_proxy_ref!(), &any_box!(hint_data)),
            Err(VirtualMachineError::MemoryError(
                MemoryError::InconsistentMemory(
                    MaybeRelocatable::from((1, 10)),
                    MaybeRelocatable::from(bigint!(0)),
                    MaybeRelocatable::from(bigint!(10)),
                )
            ))
        );
    }
}

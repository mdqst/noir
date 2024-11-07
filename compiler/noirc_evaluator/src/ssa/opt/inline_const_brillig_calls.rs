use std::collections::BTreeMap;

use fxhash::FxHashMap;
use noirc_frontend::monomorphization::ast::InlineType;

use crate::{
    errors::RuntimeError,
    ssa::{
        ir::{
            function::{Function, FunctionId, RuntimeType},
            instruction::{Instruction, TerminatorInstruction},
            value::{Value, ValueId},
        },
        opt::flatten_cfg::flatten_function_cfg,
        Ssa,
    },
};

impl Ssa {
    #[tracing::instrument(level = "trace", skip(self))]
    pub(crate) fn inline_const_brillig_calls(mut self) -> Self {
        // Collect all brillig functions so that later we can find them when processing a call instruction
        let mut brillig_functions = BTreeMap::<FunctionId, Function>::new();
        for (func_id, func) in &self.functions {
            if let RuntimeType::Brillig(..) = func.runtime() {
                let cloned_function = Function::clone_with_id(*func_id, func);
                brillig_functions.insert(*func_id, cloned_function);
            }
        }

        for func in self.functions.values_mut() {
            func.inline_const_brillig_calls(&brillig_functions);
        }
        self
    }
}

impl Function {
    pub(crate) fn inline_const_brillig_calls(
        &mut self,
        brillig_functions: &BTreeMap<FunctionId, Function>,
    ) {
        for block_id in self.reachable_blocks() {
            let block = &mut self.dfg[block_id];
            let instruction_ids = block.take_instructions();

            for instruction_id in instruction_ids {
                let instruction = &self.dfg[instruction_id];

                let Instruction::Call { func: func_id, arguments } = instruction else {
                    self.dfg[block_id].instructions_mut().push(instruction_id);
                    continue;
                };

                let func_value = &self.dfg[*func_id];
                let Value::Function(func_id) = func_value else {
                    self.dfg[block_id].instructions_mut().push(instruction_id);
                    continue;
                };

                let Some(function) = brillig_functions.get(func_id) else {
                    self.dfg[block_id].instructions_mut().push(instruction_id);
                    continue;
                };

                if !arguments.iter().all(|argument| self.dfg.is_constant(*argument)) {
                    self.dfg[block_id].instructions_mut().push(instruction_id);
                    continue;
                }

                // The function we have is already a copy of the original function, but we need to clone
                // it again because there might be multiple calls to the same brillig function.
                let mut function = Function::clone_with_id(*func_id, function);

                // Find the entry block and remove its parameters
                let entry_block_id = function.entry_block();
                let entry_block = &mut function.dfg[entry_block_id];
                let entry_block_parameters = entry_block.take_parameters();

                assert_eq!(arguments.len(), entry_block_parameters.len());

                // Replace the ValueId of parameters with the ValueId of arguments
                for (parameter_id, argument_id) in entry_block_parameters.iter().zip(arguments) {
                    // Lookup the argument in the current function and insert it in the function copy
                    let new_argument_id =
                        copy_constant_to_function(self, &mut function, *argument_id);
                    function.dfg.set_value_from_id(*parameter_id, new_argument_id);
                }

                // Try to fully optimize the function. If we can't, we can't inline it's constant value.
                if optimize(&mut function).is_err() {
                    self.dfg[block_id].instructions_mut().push(instruction_id);
                    continue;
                }

                let entry_block = &mut function.dfg[entry_block_id];

                // If the entry block has instructions, we can't inline it (we need a terminator)
                if !entry_block.instructions().is_empty() {
                    self.dfg[block_id].instructions_mut().push(instruction_id);
                    continue;
                }

                let terminator = entry_block.take_terminator();
                let TerminatorInstruction::Return { return_values, call_stack: _ } = terminator
                else {
                    self.dfg[block_id].instructions_mut().push(instruction_id);
                    continue;
                };

                // Sanity check: make sure all returned values are constant
                if !return_values.iter().all(|value_id| function.dfg.is_constant(*value_id)) {
                    self.dfg[block_id].instructions_mut().push(instruction_id);
                    continue;
                }

                // Replace the instruction results with the constant values we got
                let current_results = self.dfg.instruction_results(instruction_id).to_vec();
                assert_eq!(return_values.len(), current_results.len());

                for (current_result_id, return_value_id) in
                    current_results.iter().zip(return_values)
                {
                    let new_return_value_id =
                        copy_constant_to_function(&function, self, return_value_id);
                    self.dfg.set_value_from_id(*current_result_id, new_return_value_id);
                }
            }
        }
    }
}

fn copy_constant_to_function(
    from_function: &Function,
    to_function: &mut Function,
    argument_id: ValueId,
) -> ValueId {
    if let Some((constant, typ)) = from_function.dfg.get_numeric_constant_with_type(argument_id) {
        to_function.dfg.make_constant(constant, typ)
    } else if let Some((constants, typ)) = from_function.dfg.get_array_constant(argument_id) {
        let new_constants = constants
            .iter()
            .map(|constant_id| copy_constant_to_function(from_function, to_function, *constant_id))
            .collect();
        to_function.dfg.make_array(new_constants, typ)
    } else {
        unreachable!("A constant should be either a numeric constant or an array constant")
    }
}

/// Optimizes a function by running the same passes as `optimize_into_acir`
/// after the `inline_const_brillig_calls` pass.
/// The function is changed to be an ACIR function so the function can potentially
/// be optimized into a single return terminator.
fn optimize(function: &mut Function) -> Result<(), RuntimeError> {
    function.set_runtime(RuntimeType::Acir(InlineType::InlineAlways));

    function.mem2reg();
    function.simplify_function();
    function.as_slice_optimization();
    function.evaluate_static_assert_and_assert_constant()?;

    let mut errors = Vec::new();
    function.try_to_unroll_loops(&mut errors);
    if !errors.is_empty() {
        return Err(errors.swap_remove(0));
    }

    function.simplify_function();

    let mut no_predicates = FxHashMap::default();
    no_predicates.insert(function.id(), function.is_no_predicates());
    flatten_function_cfg(function, &no_predicates);

    function.remove_bit_shifts();
    function.mem2reg();
    function.remove_if_else();
    function.constant_fold(false);
    function.remove_enable_side_effects();
    function.constant_fold(true);
    function.dead_instruction_elimination(true);
    function.simplify_function();
    function.array_set_optimization();

    Ok(())
}

//! This pass tries to inline calls to brillig functions that have all constant arguments.
use std::collections::{BTreeMap, HashSet};

use acvm::acir::circuit::ErrorSelector;
use noirc_frontend::{monomorphization::ast::InlineType, Type};

use crate::{
    errors::RuntimeError,
    ssa::{
        ir::{
            function::{Function, FunctionId, RuntimeType},
            instruction::{Instruction, InstructionId, TerminatorInstruction},
            value::{Value, ValueId},
        },
        optimize_ssa_after_inline_const_brillig_calls, Ssa, SsaBuilder,
    },
};

impl Ssa {
    #[tracing::instrument(level = "trace", skip(self))]
    pub(crate) fn inline_const_brillig_calls(mut self, inliner_aggressiveness: i64) -> Self {
        let error_selector_to_type = &self.error_selector_to_type;

        // Collect all brillig functions so that later we can find them when processing a call instruction
        let mut brillig_functions = BTreeMap::<FunctionId, Function>::new();
        for (func_id, func) in &self.functions {
            if let RuntimeType::Brillig(..) = func.runtime() {
                let cloned_function = Function::clone_with_id(*func_id, func);
                brillig_functions.insert(*func_id, cloned_function);
            };
        }

        // Keep track of which brillig functions we couldn't completely inline: we'll remove the ones we could.
        let mut brillig_functions_we_could_not_inline = HashSet::new();

        for func in self.functions.values_mut() {
            func.inline_const_brillig_calls(
                &brillig_functions,
                &mut brillig_functions_we_could_not_inline,
                inliner_aggressiveness,
                error_selector_to_type,
            );
        }

        // Remove the brillig functions that are no longer called
        for func_id in brillig_functions.keys() {
            // We never want to remove the main function (it could be `unconstrained` or it
            // could have been turned into brillig if `--force-brillig` was given)
            if self.main_id == *func_id {
                continue;
            }

            if brillig_functions_we_could_not_inline.contains(func_id) {
                continue;
            }

            // We also don't want to remove entry points
            if self.entry_point_to_generated_index.contains_key(func_id) {
                continue;
            }

            self.functions.remove(func_id);
        }

        self
    }
}

/// Result of trying to optimize an instruction (any instruction) in this pass.
enum OptimizeResult {
    /// Nothing was done because the instruction wasn't a call to a brillig function,
    /// or some arguments to it were not constants.
    NotABrilligCall,
    /// The instruction was a call to a brillig function, but we couldn't optimize it.
    CannotOptimize(FunctionId),
    /// The instruction was a call to a brillig function and we were able to optimize it,
    /// returning the optimized function and the constant values it returned.
    Optimized(Function, Vec<ValueId>),
}

impl Function {
    pub(crate) fn inline_const_brillig_calls(
        &mut self,
        brillig_functions: &BTreeMap<FunctionId, Function>,
        brillig_functions_we_could_not_inline: &mut HashSet<FunctionId>,
        inliner_aggressiveness: i64,
        error_selector_to_type: &BTreeMap<ErrorSelector, Type>,
    ) {
        for block_id in self.reachable_blocks() {
            for instruction_id in self.dfg[block_id].take_instructions() {
                let optimize_result = self.optimize_const_brillig_call(
                    instruction_id,
                    brillig_functions,
                    brillig_functions_we_could_not_inline,
                    inliner_aggressiveness,
                    error_selector_to_type,
                );
                match optimize_result {
                    OptimizeResult::NotABrilligCall => {
                        self.dfg[block_id].instructions_mut().push(instruction_id);
                    }
                    OptimizeResult::CannotOptimize(func_id) => {
                        self.dfg[block_id].instructions_mut().push(instruction_id);
                        brillig_functions_we_could_not_inline.insert(func_id);
                    }
                    OptimizeResult::Optimized(function, return_values) => {
                        // Replace the instruction results with the constant values we got
                        let current_results = self.dfg.instruction_results(instruction_id).to_vec();
                        assert_eq!(return_values.len(), current_results.len());

                        for (current_result_id, return_value_id) in
                            current_results.iter().zip(return_values)
                        {
                            let new_return_value_id =
                                function.copy_constant_to_function(return_value_id, self);
                            self.dfg.set_value_from_id(*current_result_id, new_return_value_id);
                        }
                    }
                }
            }
        }
    }

    /// Tries to optimize an instruction if it's a call that points to a brillig function,
    /// and all its arguments are constant.
    fn optimize_const_brillig_call(
        &mut self,
        instruction_id: InstructionId,
        brillig_functions: &BTreeMap<FunctionId, Function>,
        brillig_functions_we_could_not_inline: &mut HashSet<FunctionId>,
        inliner_aggressiveness: i64,
        error_selector_to_type: &BTreeMap<ErrorSelector, Type>,
    ) -> OptimizeResult {
        let instruction = &self.dfg[instruction_id];
        let Instruction::Call { func: func_id, arguments } = instruction else {
            return OptimizeResult::NotABrilligCall;
        };

        let func_value = &self.dfg[*func_id];
        let Value::Function(func_id) = func_value else {
            return OptimizeResult::NotABrilligCall;
        };
        let func_id = *func_id;

        let Some(function) = brillig_functions.get(&func_id) else {
            return OptimizeResult::NotABrilligCall;
        };

        if !arguments.iter().all(|argument| self.dfg.is_constant(*argument)) {
            return OptimizeResult::CannotOptimize(func_id);
        }

        // The function we have is already a copy of the original function, but we need to clone
        // it again because there might be multiple calls to the same brillig function.
        let mut function = Function::clone_with_id(func_id, function);

        // Find the entry block and remove its parameters
        let entry_block_id = function.entry_block();
        let entry_block = &mut function.dfg[entry_block_id];
        let entry_block_parameters = entry_block.take_parameters();

        assert_eq!(arguments.len(), entry_block_parameters.len());

        // Replace the ValueId of parameters with the ValueId of arguments
        for (parameter_id, argument_id) in entry_block_parameters.iter().zip(arguments) {
            // Lookup the argument in the current function and insert it in the function copy
            let new_argument_id = self.copy_constant_to_function(*argument_id, &mut function);
            function.dfg.set_value_from_id(*parameter_id, new_argument_id);
        }

        // Try to fully optimize the function. If we can't, we can't inline it's constant value.
        let Ok(mut function) = optimize(function, inliner_aggressiveness, error_selector_to_type)
        else {
            return OptimizeResult::CannotOptimize(func_id);
        };

        let entry_block = &mut function.dfg[entry_block_id];

        // If the entry block has instructions, we can't inline it (we need a terminator)
        if !entry_block.instructions().is_empty() {
            brillig_functions_we_could_not_inline.insert(func_id);
            return OptimizeResult::CannotOptimize(func_id);
        }

        let terminator = entry_block.take_terminator();
        let TerminatorInstruction::Return { return_values, call_stack: _ } = terminator else {
            return OptimizeResult::CannotOptimize(func_id);
        };

        // Sanity check: make sure all returned values are constant
        if !return_values.iter().all(|value_id| function.dfg.is_constant(*value_id)) {
            return OptimizeResult::CannotOptimize(func_id);
        }

        OptimizeResult::Optimized(function, return_values)
    }

    /// Copies a constant from this function to another one.
    /// Though it might seem we can just take a value out of `self` and call `make_value` on `function`,
    /// if the constant is an array the values will still keep pointing to `self`. So, this function
    /// recursively copies the array values too.
    fn copy_constant_to_function(&self, constant_id: ValueId, function: &mut Function) -> ValueId {
        if let Some((constant, typ)) = self.dfg.get_numeric_constant_with_type(constant_id) {
            function.dfg.make_constant(constant, typ)
        } else if let Some((constants, typ)) = self.dfg.get_array_constant(constant_id) {
            let new_constants = constants
                .iter()
                .map(|constant_id| self.copy_constant_to_function(*constant_id, function))
                .collect();
            function.dfg.make_array(new_constants, typ)
        } else {
            unreachable!("A constant should be either a numeric constant or an array constant")
        }
    }
}

/// Optimizes a function by running the same passes as `optimize_into_acir`
/// after the `inline_const_brillig_calls` pass.
/// The function is changed to be an ACIR function so the function can potentially
/// be optimized into a single return terminator.
fn optimize(
    mut function: Function,
    inliner_aggressiveness: i64,
    error_selector_to_type: &BTreeMap<ErrorSelector, Type>,
) -> Result<Function, RuntimeError> {
    function.set_runtime(RuntimeType::Acir(InlineType::InlineAlways));

    let ssa = Ssa::new(vec![function], error_selector_to_type.clone());
    let builder = SsaBuilder { ssa, print_ssa_passes: false, print_codegen_timings: false };
    let mut ssa = optimize_ssa_after_inline_const_brillig_calls(
        builder,
        inliner_aggressiveness,
        // Don't inline functions with no predicates.
        // The reason for this is that in this specific context the `Ssa` object only holds
        // a single function. For inlining to work we need to know all other functions that
        // exist (so we can inline them). Here we choose to skip this optimization for simplicity reasons.
        false,
    )?;
    Ok(ssa.functions.pop_first().unwrap().1)
}

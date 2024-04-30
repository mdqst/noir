mod errors;
mod hir_to_ast;
mod interpreter;
mod scan;
mod tests;
mod value;

pub use errors::{IResult, InterpreterError};
pub use interpreter::{Interpreter, InterpreterState};
pub use value::Value;
use core::ffi::c_void;
use core::fmt::{self, Debug};
use core::mem::transmute;

use crate::erts::process::FrameWithArguments;
use crate::erts::term::closure::Definition;
use crate::erts::term::prelude::*;
use crate::erts::ModuleFunctionArity;
use crate::Arity;

use super::ffi::ErlangResult;

#[derive(Clone)]
pub struct Frame {
    definition: Definition,
    module_function_arity: ModuleFunctionArity,
    native: Native,
}

impl Frame {
    pub fn new(module_function_arity: ModuleFunctionArity, native: Native) -> Frame {
        Frame {
            definition: Definition::Export {
                function: module_function_arity.function,
            },
            module_function_arity,
            native,
        }
    }

    pub fn from_definition(
        module: Atom,
        definition: Definition,
        arity: u8,
        native: Native,
    ) -> Frame {
        Frame {
            module_function_arity: ModuleFunctionArity {
                module,
                function: definition.function_name(),
                arity,
            },
            definition,
            native,
        }
    }

    pub fn module_function_arity(&self) -> ModuleFunctionArity {
        self.module_function_arity
    }

    pub fn definition(&self) -> &Definition {
        &self.definition
    }

    pub fn native(&self) -> Native {
        self.native
    }

    pub fn with_arguments(&self, uses_returned: bool, arguments: &[Term]) -> FrameWithArguments {
        FrameWithArguments::new(self.clone(), uses_returned, arguments)
    }
}

impl Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Frame")
            .field("module_function_arity", &self.module_function_arity)
            .field("native", &self.native)
            .finish()
    }
}

#[derive(Copy, Clone)]
pub enum Native {
    Zero(extern "C-unwind" fn() -> ErlangResult),
    One(extern "C-unwind" fn(Term) -> ErlangResult),
    Two(extern "C-unwind" fn(Term, Term) -> ErlangResult),
    Three(extern "C-unwind" fn(Term, Term, Term) -> ErlangResult),
    Four(extern "C-unwind" fn(Term, Term, Term, Term) -> ErlangResult),
    Five(extern "C-unwind" fn(Term, Term, Term, Term, Term) -> ErlangResult),
}

impl Native {
    pub unsafe fn from_ptr(ptr: *const c_void, arity: Arity) -> Self {
        match arity {
            0 => Self::Zero(transmute::<_, extern "C-unwind" fn() -> ErlangResult>(ptr)),
            1 => Self::One(transmute::<_, extern "C-unwind" fn(Term) -> ErlangResult>(
                ptr,
            )),
            2 => Self::Two(transmute::<
                _,
                extern "C-unwind" fn(Term, Term) -> ErlangResult,
            >(ptr)),
            3 => Self::Three(transmute::<
                _,
                extern "C-unwind" fn(Term, Term, Term) -> ErlangResult,
            >(ptr)),
            4 => Self::Four(transmute::<
                _,
                extern "C-unwind" fn(Term, Term, Term, Term) -> ErlangResult,
            >(ptr)),
            5 => Self::Five(transmute::<
                _,
                extern "C-unwind" fn(Term, Term, Term, Term, Term) -> ErlangResult,
            >(ptr)),
            _ => unimplemented!(
                "Converting `*const c_void` ptr with arity {} to `fn`",
                arity
            ),
        }
    }

    pub fn apply(&self, arguments: &[Term]) -> ErlangResult {
        match self {
            Self::Zero(f) => {
                assert_eq!(arguments.len(), 0);
                f()
            }
            Self::One(f) => {
                assert_eq!(arguments.len(), 1);
                f(arguments[0])
            }
            Self::Two(f) => {
                assert_eq!(arguments.len(), 2);
                f(arguments[0], arguments[1])
            }
            Self::Three(f) => {
                assert_eq!(arguments.len(), 3);
                f(arguments[0], arguments[1], arguments[2])
            }
            Self::Four(f) => {
                assert_eq!(arguments.len(), 4);
                f(arguments[0], arguments[1], arguments[2], arguments[3])
            }
            Self::Five(f) => {
                assert_eq!(arguments.len(), 5);
                f(
                    arguments[0],
                    arguments[1],
                    arguments[2],
                    arguments[3],
                    arguments[4],
                )
            }
        }
    }

    pub fn arity(&self) -> Arity {
        match self {
            Self::Zero(_) => 0,
            Self::One(_) => 1,
            Self::Two(_) => 2,
            Self::Three(_) => 3,
            Self::Four(_) => 4,
            Self::Five(_) => 5,
        }
    }

    pub fn ptr(&self) -> *const c_void {
        match *self {
            Self::Zero(ptr) => ptr as *const c_void,
            Self::One(ptr) => ptr as *const c_void,
            Self::Two(ptr) => ptr as *const c_void,
            Self::Three(ptr) => ptr as *const c_void,
            Self::Four(ptr) => ptr as *const c_void,
            Self::Five(ptr) => ptr as *const c_void,
        }
    }
}

impl Debug for Native {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:x}/{}", self.ptr() as usize, self.arity())
    }
}

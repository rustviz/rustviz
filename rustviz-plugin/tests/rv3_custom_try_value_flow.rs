#![feature(try_trait_v2)]

use std::convert::Infallible;
use std::ops::{ControlFlow, FromResidual, Try};

struct Pending(bool);

impl FromResidual<Result<Infallible, ()>> for Pending {
    fn from_residual(_: Result<Infallible, ()>) -> Self {
        Self(false)
    }
}

impl Try for Pending {
    type Output = String;
    type Residual = Result<Infallible, ()>;

    fn from_output(_: String) -> Self {
        Self(true)
    }

    fn branch(self) -> ControlFlow<Self::Residual, String> {
        if self.0 {
            ControlFlow::Continue(String::from("generated"))
        } else {
            ControlFlow::Break(Err(()))
        }
    }
}

fn wrap(pending: Pending) -> Result<String, ()> {
    Ok(pending?)
}

fn nested(pending: Pending) -> Result<Option<String>, ()> {
    Ok(Some(pending?))
}

fn main() {
    assert_eq!(wrap(Pending(true)), Ok(String::from("generated")));
    assert_eq!(wrap(Pending(false)), Err(()));
    assert_eq!(nested(Pending(true)), Ok(Some(String::from("generated"))));
    assert_eq!(nested(Pending(false)), Err(()));
}

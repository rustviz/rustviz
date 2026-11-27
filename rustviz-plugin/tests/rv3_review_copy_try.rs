#![feature(try_trait_v2)]

use std::convert::Infallible;
use std::ops::{ControlFlow, FromResidual, Try};

#[derive(Clone, Copy)]
struct CopyPending(bool);

impl FromResidual<Result<Infallible, ()>> for CopyPending {
    fn from_residual(_: Result<Infallible, ()>) -> Self { Self(false) }
}

impl Try for CopyPending {
    type Output = String;
    type Residual = Result<Infallible, ()>;

    fn from_output(_: String) -> Self { Self(true) }
    fn branch(self) -> ControlFlow<Self::Residual, String> {
        if self.0 {
            ControlFlow::Continue(String::from("fresh output"))
        } else {
            ControlFlow::Break(Err(()))
        }
    }
}

fn extract(pending: CopyPending) -> Result<String, ()> {
    let text = pending?;
    assert!(pending.0);
    Ok(text)
}

fn main() {
    assert_eq!(extract(CopyPending(true)).unwrap(), "fresh output");
    assert!(extract(CopyPending(false)).is_err());
}

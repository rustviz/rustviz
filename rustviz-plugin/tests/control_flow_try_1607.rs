use std::ops::ControlFlow;

fn run(o: ControlFlow<()>) -> ControlFlow<()> {
    let s = String::from("a");
    o?;
    ControlFlow::Continue(())
}

fn owned(input: ControlFlow<String, String>) -> ControlFlow<String> {
    let value = input?;
    drop(value);
    ControlFlow::Continue(())
}

fn main() {
    let _ = owned(ControlFlow::Continue(String::new()));
    let _ = run(ControlFlow::Break(()));
    let _ = run(ControlFlow::Continue(()));
}

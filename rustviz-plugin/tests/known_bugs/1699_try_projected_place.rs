// Known bug, from the closed 2.x fix PRs (cx/hedge-t1); kept as a RustViz 3 regression case.
// Issue(s): #1699 (see known_bugs/README.md for what main draws wrong).
use std::task::Poll;

fn moved_box(result: Result<String, String>) -> Result<usize, String> {
    let moved = Box::new(result);
    let text = (*moved)?;
    Ok(text.len())
}

fn moved_nested(result: Result<String, String>) -> Result<usize, String> {
    let nested = Box::new(Box::new(result));
    let text = (**nested)?;
    Ok(text.len())
}

fn poll_box(value: Poll<Result<String, String>>) -> Poll<Result<usize, String>> {
    let polled = Box::new(value);
    let output = (*polled)?;
    Poll::Ready(Ok(match output {
        Poll::Ready(text) => text.len(),
        Poll::Pending => 0,
    }))
}

fn poll_place(value: Poll<Result<String, String>>) -> Poll<Result<usize, String>> {
    let p = value;
    let output = p?;
    Poll::Ready(Ok(match output {
        Poll::Ready(text) => text.len(),
        Poll::Pending => 0,
    }))
}

fn consume_copy(n: i32) -> i32 {
    n
}

fn copy_payloads(shared: &Option<i32>) -> Option<i32> {
    let b = Box::new(Some(10));
    let x = (*b)?;
    let nested_copy = Box::new(Box::new(Some(20)));
    let y;
    y = (**nested_copy)?;
    let z = (*shared)?;
    Some(x + y + z + consume_copy((*b)?))
}

fn projected_in_branch(flag: bool, value: Result<String, String>) -> Result<(), String> {
    let maybe_nested = Box::new(Box::new(value));
    if flag {
        drop((**maybe_nested)?);
    }
    Ok(())
}

fn projected_return(ret: &Option<Option<i32>>) -> Option<i32> {
    (*ret)?
}

fn copied_poll(value: Poll<Result<i32, i32>>) -> Poll<Result<Poll<i32>, i32>> {
    let poll_copy = Box::new(value);
    let poll_output = (*poll_copy)?;
    Poll::Ready(Ok(poll_output))
}

fn main() {
    assert_eq!(moved_box(Err(String::from("stop"))), Err(String::from("stop")));
    assert_eq!(moved_nested(Ok(String::from("hi"))), Ok(2));
    assert_eq!(poll_box(Poll::Pending), Poll::Ready(Ok(0)));
    assert_eq!(poll_place(Poll::Pending), Poll::Ready(Ok(0)));
    assert_eq!(copy_payloads(&Some(1)), Some(41));
}

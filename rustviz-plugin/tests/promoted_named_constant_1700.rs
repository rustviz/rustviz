const N: i32 = 1;

fn main() {
    let q: &'static Result<i32, String> = &Ok(N);
    let n = <String as From<&str>>::from("one");
    let v = Vec::<String>::new();
    assert!(q.is_ok());
}

fn requires_static(_: &'static Result<i32, String>) {}

fn other_borrows(left: bool) {
    let branch = if left { &Ok(N) } else { &Ok(N + 1) };
    let tuple: (&Result<i32, String>,) = (&Ok(N),);
    requires_static(branch);
    requires_static(tuple.0);
    requires_static(&Ok(N));
}

fn runtime_value(message: String) {
    let local: &Result<i32, String> = &Err(message);
    assert!(local.is_err());
}

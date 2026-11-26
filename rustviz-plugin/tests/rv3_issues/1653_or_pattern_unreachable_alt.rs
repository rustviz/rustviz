fn after_let() {
    let mut u = (String::from("u0"), String::from("u1"));
    let ((y, _) | (_, y)) = u;      // rustc: warning, unreachable pattern (second alternative)
    println!("{}", y);
    u = (String::from("u2"), String::from("u3"));
}

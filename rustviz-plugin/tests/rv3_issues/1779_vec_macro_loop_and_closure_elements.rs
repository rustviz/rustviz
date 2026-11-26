fn loop_element() {
    let loop_s = String::from("s");
    let lv = vec![loop_s, format!("x"), loop { break String::from("done"); }];
    println!("{}", lv.len());   // loop_s is drawn "owner" to its scope end: its move into lv isn't drawn
}
fn closure_call_element() {
    let call_s = String::from("s");
    let cv = vec![format!("first"), (|| -> String { panic!("stop") })(), call_s];
    // "call_s goes out of scope. No resource is dropped.": the panic happens before call_s is reached,
    // so call_s is dropped during unwinding
    println!("{}", cv.len());
}

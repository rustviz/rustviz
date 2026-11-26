fn slot_case() {
    let a = String::from("a");
    let mut r = &a;
    let slot: &mut &String = &mut r;
    let mut u: Vec<&mut &String> = Vec::new();
    u.push(slot);
    println!("{a} {u:?}");
}

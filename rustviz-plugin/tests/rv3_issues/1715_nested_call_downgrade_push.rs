fn downgrade(s: &mut i32) -> &i32 { s }
fn id(s: &i32) -> &i32 { s }
fn five() {
    let mut a5 = 1;
    let mut v5: Vec<&i32> = Vec::new();
    v5.push(id(downgrade(&mut a5)));
    println!("{:?}", v5);
}

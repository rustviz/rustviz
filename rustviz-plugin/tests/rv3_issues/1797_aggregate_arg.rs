fn pick<'a>(xs: [&'a i32; 2]) -> Option<&'a i32> { if *xs[0] > 5 { Some(xs[0]) } else { None } }
fn f() {
    let a = 1;
    let b = 2;
    let r = pick([&a, &b]);
    println!("{r:?}");
}

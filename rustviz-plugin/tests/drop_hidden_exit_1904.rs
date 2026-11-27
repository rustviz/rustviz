pub struct Pair {
    a: String,
    b: String,
}

fn helper() {} // rustviz: hide

pub fn hidden_exit(p: Pair) {
    drop(p.a);
    std::process::exit(0);
}

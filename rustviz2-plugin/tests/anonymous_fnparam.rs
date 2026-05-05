fn main () {
    let y = inc(five());
    let c = inc_by_ref(&five());
}

fn five() -> i32 {
    5
}

fn inc(z: i32) -> i32{
    z + 1
}

fn inc_by_ref(x: &i32) -> i32 {
    x + 1
}
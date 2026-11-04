// #191: assigning a whole union (`u = Pair { .. }`) drops nothing (a
// union without its own Drop impl has no drop glue), and a field write
// with a variable on the right (`u.a = y`) copies y into u.
union Pair { a: i32, b: f32 }

fn main() {
    let mut u = Pair { a: 1 };
    let y = 3;
    u.a = y;
    u = Pair { b: 2.0 };
    let k = unsafe { u.a };
    println!("{}", k);
}

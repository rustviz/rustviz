// Calling a stdlib immutable-ref method (`len` is `&self`).
// Should produce a PassByStaticReference ("len reads from s").

fn main() {
    let s = String::from("hi");
    let n = s.len();
    println!("{} {}", s, n);
}

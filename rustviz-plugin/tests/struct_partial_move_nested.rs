// Partial moves out of a nested struct (#207): moving `o.inner.x`
// leaves `o.inner.y` and `o.z` to be dropped; moving all of `q.inner`
// leaves only `q.z`.
struct In { x: String, y: String }
struct Out { inner: In, z: String }
fn main() {
    let o = Out { inner: In { x: String::from("x"), y: String::from("y") }, z: String::from("z") };
    let a = o.inner.x;
    let q = Out { inner: In { x: String::from("x"), y: String::from("y") }, z: String::from("z") };
    let i = q.inner;
    println!("{} {}", a, i.y);
}

// Regression for: tooltips for Copy-typed owners (i32 etc.) used the
// "ownership" framing ("n acquires ownership of a resource", "n is
// the owner of the resource") even though primitives don't have
// heap-resource semantics. The user spotted this on `n += 1` inside
// a while loop. Now the dot tooltip reads "n is bound to a value"
// and the lifeline tooltip reads "n holds a value".
fn read(_n: i32) {}

fn main() {
    let mut n = 0;
    while n < 3 {
        read(n);
        n += 1;
    }
}

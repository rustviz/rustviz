#[derive(Debug)]
struct H<'a> { r: &'a mut i32 }
fn main() {
    let mut n = 0;
    let h = H { r: &mut n };
    let mut f = || { println!("{:?}", h); *h.r += 1; };
    f();
}

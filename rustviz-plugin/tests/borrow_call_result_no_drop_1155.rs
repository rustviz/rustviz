struct P { a: i32 }
struct H<'a> { r: &'a i32 }
fn compute() -> i32 { 5 }
fn make() -> P { P { a: 1 } }
fn arr() -> [i32; 2] { [1, 2] }
fn main() {
    let r = &compute();
    let m = &mut compute();
    *m += 1;
    let p = &make();
    let pa = &make().a;
    let x = &arr()[1];
    let h = H { r: &compute() };
    let t = (&compute(), 1);
    println!("{} {} {} {} {} {} {}", r, m, p.a, pa, x, h.r, t.0);
    let v = vec![1, 2, 3];
    let it = &mut v.iter();
    println!("{:?}", it.next());
}

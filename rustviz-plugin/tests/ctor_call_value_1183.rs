// #1183: `let x = Some(one());` holds a Some value around the value one()
// returns, not that value itself.
fn one() -> i32 { 1 }
fn main() {
    let _x = Some(one());
    let h = || 2;
    let _y = Some(h());
    let _p: Result<i32, ()> = Ok(one());
    let _f = Some(5);
}

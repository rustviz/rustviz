#![allow(non_snake_case)]

unsafe fn raw<'a>(p: *const i32) -> &'a i32 {
    unsafe { &*p }
}
fn buffer(s: &mut String) -> &str {
    s
}
fn Make() -> u16 { 300 }
fn read_cast() -> u8 {
    let n = Make() as u8;
    n
}
static SOURCE: i32 = 7;
fn MakeRef() -> Option<&'static i32> { Some(&SOURCE) }
fn read_pattern() {
    if let Some(&n) = MakeRef() {
        assert_eq!(n, 7);
    }
}
fn answer() -> usize { 42 }
fn tuple() -> (usize, bool) {
    (answer(), false)
}

struct Observed(&'static str);
impl Drop for Observed {
    fn drop(&mut self) { println!("{}", self.0); }
}
enum Out {
    Pair(Observed, Observed),
    Fail,
}
impl From<()> for Out {
    fn from(_: ()) -> Self { Self::Fail }
}
fn both(a: Result<Observed, ()>, b: Result<Observed, ()>) -> Result<Out, ()> {
    Ok(Out::Pair(a?, b?))
}
fn short_circuit() {
    assert!(both(Ok(Observed("first successful payload")), Err(())).is_err());
    println!("residual received");
}

fn main() {
    let place = 42;
    assert_eq!(*unsafe { raw(&place) }, 42);
    assert_eq!(buffer(&mut String::from("data")), "data");
    assert_eq!(read_cast(), 44);
    read_pattern();
    assert_eq!(tuple(), (42, false));
    short_circuit();
}

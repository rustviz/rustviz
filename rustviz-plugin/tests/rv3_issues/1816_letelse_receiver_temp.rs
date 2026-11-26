struct Source;
impl Source { fn get(&self) -> Option<u32> { None } }
impl Drop for Source { fn drop(&mut self) { println!("source dropped"); } }
fn f() {
    let Some(x) = Source.get() else {
        println!("leaving");
        return;
    };
    println!("{}", x);
}
fn main() { f(); }

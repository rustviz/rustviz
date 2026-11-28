// A ref pattern borrows the payload for this pass after the collection moves.
fn ref_pattern(words: Vec<String>) {
    for ref word in words {
        assert_eq!(word, "hello");
    }
}

struct Wrapped(i32);
impl<'a> IntoIterator for &'a mut Wrapped {
    type Item = &'a mut i32;
    type IntoIter = std::iter::Once<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        std::iter::once(Box::leak(Box::new(10)))
    }
}

fn produce(_: &mut i32) -> Vec<&mut i32> {
    vec![Box::leak(Box::new(20))]
}

fn opaque() {
    let mut wrapped = Wrapped(1);
    for item in &mut wrapped {
        *item = 0;
    }
    assert_eq!(wrapped.0, 1);
    let mut x = 2;
    for cell in produce(&mut x) {
        *cell = 0;
    }
    assert_eq!(x, 2);
}

struct Maker(i32);
impl IntoIterator for &mut Maker {
    type Item = (i32, i32);
    type IntoIter = std::iter::Once<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        std::iter::once((1, 2))
    }
}

fn independent(mut maker: Maker) {
    for (a, b) in &mut maker {
        let again = &mut maker;
        again.0 = a + b;
    }
}

fn replacement(v: &mut Vec<i32>, spare: &mut i32) {
    for mut item in &mut *v {
        *item = 1;
        item = &mut *spare;
        drop(item);
    }
}

fn branches(flag: bool, a: Vec<i32>, b: Vec<i32>) {
    for item in if flag { a.iter() } else { b.iter() } {
        std::hint::black_box(item);
    }
}

struct Trace(&'static str);
impl Drop for Trace {
    fn drop(&mut self) { println!("{}", self.0); }
}

fn temporary(flag: bool) {
    for word in vec![Trace("item"), Trace("rest")] {
        if flag {
            return;
        }
    }
}

fn partial(flag: bool) {
    let iter = [(Trace("first"), Trace("second"))].into_iter();
    for pair in iter {
        drop(pair.0);
        if flag {
            return;
        }
    }
}

fn result_input(input: Result<String, Trace>) {
    for value in input {
        println!("{value}");
    }
    println!("after conversion");
}

fn main() {
    ref_pattern(vec![String::from("hello")]);
    opaque();
    independent(Maker(0));
    replacement(&mut vec![0], &mut 2);
    branches(true, vec![1], vec![]);
    temporary(true);
    partial(true);
    result_input(Err(Trace("error")));
}

struct LoanMaker<'a>(&'a i32);
impl<'a> IntoIterator for &mut LoanMaker<'a> {
    type Item = &'a i32;
    type IntoIter = std::iter::Once<Self::Item>;
    fn into_iter(self) -> Self::IntoIter { std::iter::once(self.0) }
}

fn distinct_head_loan() {
    let n = 1;
    let mut maker = LoanMaker(&n);
    for item in &mut maker {
        let again = &mut maker;
        again.0 = &n;
        println!("{item}");
    }
}

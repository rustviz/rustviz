struct Holder<'a> {
    value: &'a String,
}

impl Drop for Holder<'_> {
    fn drop(&mut self) {
        println!("{}", self.value);
    }
}

fn main() {
    let first = String::from("first");
    let second = String::from("second");
    let mut h = Holder { value: &first };
    h = Holder { value: &second };
    println!("{}", h.value);
}

struct T { a: Vec<i32>, b: Vec<i32> }
impl T {
    fn f(&mut self) {
        for (x, y) in self.a.iter().copied().collect::<Vec<_>>().into_iter().zip(self.b.iter()) {
            self.a.clear();
            println!("{x} {y}");
        }
    }
}

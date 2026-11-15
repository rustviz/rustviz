struct Bag {
    items: Vec<i32>,
    n: i32,
}
impl Bag {
    fn bump(&mut self) {
        for e in self.items.iter_mut() {
            *e += 1;
        }
        self.items.push(0);
    }
    fn total(&mut self) -> i32 {
        let mut t = 0;
        for x in self.items.iter() {
            t += *x;
        }
        self.n = t;
        t
    }
}
fn read_only(v: &mut Vec<i32>) {
    for x in v.iter() {
        println!("{}", x);
    }
    v.push(1);
}
fn through_local_ref() {
    let mut s = vec![1, 2];
    let r = &mut s;
    for x in r.iter() {
        println!("{}", x);
    }
    r.push(3);
}
fn main() {
    let mut b = Bag { items: vec![1], n: 0 };
    b.bump();
    b.total();
    let mut nums = vec![1];
    read_only(&mut nums);
    through_local_ref();
}

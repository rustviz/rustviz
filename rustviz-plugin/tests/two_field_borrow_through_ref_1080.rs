// #1080: a value or reference that borrows several fields of what a
// `&mut` reference points to restricts only those fields. In each
// function below another field (`n`) is written while the borrow is
// live, and rustc accepts it; using a borrowed field there would not
// compile (`self.items.push(0)` / `self.other.push(0)` after `let it`
// in `both` are E0502; `r.other.len()` in `add_into` is E0502 and
// `r.other.push(0)` E0499, while `r.items.len()` compiles). `other`
// in `skip_n` is borrowed only for the `len()` call
// (`self.other.push(0)` after it compiles), and `self.n` there is
// copied.
struct Bag {
    items: Vec<i32>,
    other: Vec<i32>,
    n: usize,
}

fn pick<'a>(a: &'a Vec<i32>, b: &'a Vec<i32>) -> &'a Vec<i32> {
    if a.len() >= b.len() { a } else { b }
}

fn first_of<'a>(a: &'a mut Vec<i32>, b: &'a mut Vec<i32>) -> Option<&'a mut i32> {
    if a.is_empty() { b.first_mut() } else { a.first_mut() }
}

impl Bag {
    fn both(&mut self) -> usize {
        let it = self.items.iter().chain(self.other.iter());
        self.n += 1;
        it.count()
    }

    fn one(&mut self) -> usize {
        let it = self.items.iter();
        self.n += 1;
        it.count()
    }

    fn longer(&mut self) -> usize {
        let l = pick(&self.items, &self.other);
        self.n += 1;
        l.len()
    }

    fn skip_n(&mut self) -> i32 {
        let it = self.items.iter().skip(self.n).take(self.other.len());
        self.n += 1;
        it.sum()
    }

    fn looped(&mut self) {
        for (a, b) in self.items.iter().zip(self.other.iter_mut()) {
            *b += *a;
            self.n += 1;
        }
    }

    fn peek(&mut self) {
        if let Some(x) = first_of(&mut self.items, &mut self.other) {
            self.n += 1;
            *x += 1;
        }
    }
}

fn add_into(r: &mut Bag) {
    let pairs = r.other.iter_mut().zip(r.items.iter());
    r.n += 1;
    for (o, i) in pairs {
        *o += *i;
    }
}

fn main() {
    let mut b = Bag { items: vec![1, 2], other: vec![3, 4], n: 0 };
    let c = b.both() + b.one() + b.longer();
    let s = b.skip_n();
    b.looped();
    b.peek();
    add_into(&mut b);
    println!("{} {} {}", c, s, b.n);
}

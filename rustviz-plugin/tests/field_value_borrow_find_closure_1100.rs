// #1100: a field of a Copy type (`n: usize`) reached through a reference
// holds a value, not a resource, so borrowing it reads like borrowing a
// Copy local (`let n = 0; let r = &n;`). A field whose type needs drop
// (`s: String`) holds a resource.
// #1101: `find` is passed a closure that captures `self.n`, but doesn't
// keep it: the `Option<&i32>` it returns holds only the borrow of
// `self.items`, so the closure's borrow of `self.n` ends with the call.
// While `f` is live, `self.n += 1` compiles and `self.items.push(0)`
// would be E0502, in `found` and in `counted` (whose closure writes
// `self.n`). While `r` is live in `single`, `self.n += 1` would be E0506.
struct Bag {
    items: Vec<i32>,
    s: String,
    n: usize,
}

impl Bag {
    fn single(&mut self) -> usize {
        let r = &self.n;
        self.items.push(0);
        *r
    }

    fn bump(&mut self) -> usize {
        let m = &mut self.n;
        *m += 1;
        let q = &self.s;
        q.len() + *m
    }

    fn found(&mut self) -> i32 {
        let f = self.items.iter().find(|x| **x as usize == self.n);
        self.n += 1;
        *f.unwrap_or(&0)
    }

    fn counted(&mut self) -> i32 {
        let f = self.items.iter().find(|x| {
            self.n += 1;
            **x > 0
        });
        self.n += 1;
        *f.unwrap_or(&0)
    }
}

fn read(p: &Bag) -> usize {
    let r = &p.n;
    *r
}

fn main() {
    let mut b = Bag { items: vec![1], s: String::new(), n: 0 };
    b.single();
    b.bump();
    b.found();
    b.counted();
    read(&b);
}

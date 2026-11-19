struct H<'a> { hr: &'a mut Vec<i32> }
struct Team { names: Vec<String>, size: u32 }
impl Team {
    fn print_all(self) {
        for m in self.names {
            println!("{}", m);
        }
    }
}
fn local_ref() {
    let mut v = vec![1, 2, 3];
    let r1 = &mut v;
    for x in r1 {
        *x += 1;
    }
    println!("{:?}", v);
}
fn param_ref(r2: &mut Vec<i32>) {
    for y in r2 {
        *y += 1;
    }
}
fn nested() {
    let mut grid = vec![vec![1], vec![2]];
    let r3 = &mut grid;
    for row in r3 {
        for z in row {
            *z += 1;
        }
    }
    println!("{:?}", grid);
}
fn field_ref(h: H) {
    for e in h.hr {
        *e += 1;
    }
}
fn fields() {
    let t = Team { names: vec![String::from("a")], size: 1 };
    for n in t.names {
        println!("{}", n);
    }
    println!("{}", t.size);
    let pair = (vec![String::from("b")], 1);
    for p in pair.0 {
        println!("{}", p);
    }
}
fn main() {
    local_ref();
    param_ref(&mut vec![1]);
    nested();
    let mut w = vec![1];
    field_ref(H { hr: &mut w });
    fields();
    Team { names: vec![], size: 0 }.print_all();
}

// Several references whose borrow ends on one line but whose lender has
// no column (a let-extended temporary `&compute()`, a string literal):
// each end is a short arrow drawn on the line's own row, so no empty rows
// go below that line (#1364: n of them inserted n - 1).
fn compute() -> i32 {
    5
}

fn temporaries() {
    let r = &compute();
    let m = &compute();
    let q = &compute();
    println!("{} {} {}", r, m, q);
    let v = 1;
    println!("{}", v);
}

fn literals() {
    let a = "hi";
    let b = "yo";
    println!("{} {}", a, b);
    let v = 2;
    println!("{}", v);
}

fn main() {
    temporaries();
    literals();
}

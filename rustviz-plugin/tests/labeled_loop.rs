fn read(_n: i32) {}

fn main() {
    let mut n = 0;
    'outer: loop {
        loop {
            read(n);
            n += 1;
            if n >= 3 {
                break 'outer;
            }
        }
    }
}

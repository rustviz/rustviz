fn consume(i: std::slice::Iter<i32>) -> usize { i.count() }
fn main() {
    let v = vec![1, 2, 3];
    let it = v.iter();
    let it2 = it;
    let n = consume(it2);
    println!("{} {:?}", n, v);
}

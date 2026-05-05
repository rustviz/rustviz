fn main () {
    let x = 7;
    let y = &x;
    let z = y;
    let a = z;
    let c = *a + *z + *y;
}
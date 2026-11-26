// Known bug, from the closed 2.x fix PRs (fix/cx-oracle2-1678-1679-1681-1682-1685); kept as a RustViz 3 regression case.
// Issue(s): #1678 (see known_bugs/README.md for what main draws wrong).
fn main() {
    let list = vec![1, 2, 3];
    let only_borrows = || println!("{list:?}");
    println!("{list:?}");
    only_borrows();
}

fn closure_in_branch(c: bool) {
    let branch_list = vec![4];
    let read = || branch_list.len();
    if c {
        println!("{branch_list:?}");
        read();
    }
}

fn same_row() {
    let mut same_row_list = vec![1];
    let read = || same_row_list.len();
    read(); println!("{same_row_list:?}");
    same_row_list.push(2);
}

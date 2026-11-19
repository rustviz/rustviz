// #1160: a tab-indented program. The code panel shows it as written
// (tabs as 4-column tab stops), and every name gets its own highlight.
fn main() {
	let s = String::from("hi");	// tab before a comment
	let t = s;
	if t.len() > 1 {
		let u = t;
		println!("{}", u);
	}
}

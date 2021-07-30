fn <tspan class="fn" data-hash="0" hash="7">f</tspan>(<tspan data-hash="6">s1</tspan>: &amp;String) {
    <tspan data-hash="6">s1</tspan>.<tspan class="fn" data-hash="0" hash="8">push_str</tspan>(" 490!");
}

fn main() {
    let mut <tspan data-hash="1">num</tspan> = 490;
    let mut <tspan data-hash="2">x</tspan> = <tspan class="fn" data-hash="0" hash="9">String::from</tspan>("EECS");
    {
        let <tspan data-hash="3">y</tspan> = &amp;mut <tspan data-hash="2">x</tspan>;
        <tspan class="fn" data-hash="0" hash="7">f</tspan>(<tspan data-hash="3">y</tspan>);
        let mut <tspan data-hash="4">s2</tspan> = <tspan data-hash="2">x</tspan>;
        <tspan data-hash="4">s2</tspan>.<tspan class="fn" data-hash="0" hash="8">push_str</tspan>(" Woo!");
        println!("{}", <tspan data-hash="4">s2</tspan>);

        let <tspan data-hash="5">n1</tspan> = <tspan data-hash="1">num</tspan>;
        println!("{}", <tspan data-hash="5">n1</tspan>);
    }
}
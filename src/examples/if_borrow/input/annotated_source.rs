fn main() {
    let mut <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="3">String::from</tspan>("ABC");
    let mut <tspan data-hash="2">y</tspan> = <tspan class="fn" data-hash="0" hash="3">String::from</tspan>("ABC");
    let <tspan data-hash="3">z</tspan> = <tspan data-hash="1">&amp;mut y</tspan>;
    let <tspan data-hash="4">guard</tspan> = 1;
    if <tspan data-hash="4">guard</tspan> == 1 {
        <tspan data-hash="3">z</tspan> = <tspan data-hash="1">&amp;mut x</tspan>;
        <tspan data-hash="3">z</tspan>.<tspan class="fn" data-hash="0" hash="8">push_str</tspan>(",");
    }
    else{
        <tspan data-hash="3">z</tspan>.<tspan class="fn" data-hash="0" hash="8">push_str</tspan>(",");
    }
}
fn main(){
    let mut <tspan data-hash="1">s</tspan> = <tspan class="fn" data-hash="0" hash="5">String::from</tspan>("hello");

    let <tspan data-hash="2">r1</tspan> = <tspan data-hash="1">&amp;s</tspan>;
    let <tspan data-hash="3">r2</tspan> = <tspan data-hash="1">&amp;s</tspan>;
    assert!(<tspan class="fn" data-hash="0" hash="6">compare_strings</tspan>(<tspan data-hash="2">r1</tspan>, <tspan data-hash="3">r2</tspan>));

    let <tspan data-hash="4">r3</tspan> = <tspan data-hash="1">&amp;mut s</tspan>;
    <tspan class="fn" data-hash="0" hash="7">clear_string</tspan>(<tspan data-hash="4">r3</tspan>);
}
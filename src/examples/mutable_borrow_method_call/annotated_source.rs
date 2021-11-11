fn main() { 
    let mut <tspan data-hash="1">s1</tspan> = <tspan class="fn" data-hash="0" hash="3">String::from</tspan>("Hello");
    let <tspan data-hash="2">s2</tspan> = <tspan class="fn" data-hash="0" hash="3">String::from</tspan>(", world");
    <tspan class="fn" data-hash="0" hash="4">String::push_str</tspan>(<tspan data-hash="1">&amp;mut s1</tspan>, <tspan data-hash="2">&amp;s2</tspan>); 
    <tspan data-hash="1">s1</tspan>.<tspan class="fn" data-hash="0" hash="5">push_str(<tspan data-hash="2">&amp;s2</tspan>)</tspan>; // shorthand for the above
    <tspan class="fn" data-hash="0" hash="6">println!</tspan>("{}", <tspan data-hash="1">s1</tspan>); // prints "Hello, world, world"
}

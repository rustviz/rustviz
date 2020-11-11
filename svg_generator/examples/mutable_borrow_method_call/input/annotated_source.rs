<tspan fill="#AAA">1  </tspan>fn main() { 
<tspan fill="#AAA">2  </tspan>    let mut <tspan data-hash="1">s1</tspan> = <tspan class="fn" data-hash="0" hash="5">String::from</tspan>("hello");
<tspan fill="#AAA">3  </tspan>    let <tspan data-hash="2">s2</tspan> = <tspan class="fn" data-hash="0" hash="5">String::from</tspan>(", world");
<tspan fill="#AAA">4  </tspan>    <tspan class="fn" data-hash="0" hash="6">String::push_str</tspan>(&amp;mut <tspan data-hash="1">s1</tspan>, <tspan data-hash="2">&amp;s2</tspan>); 
<tspan fill="#AAA">5  </tspan>    <tspan data-hash="1">s1</tspan>.<tspan class="fn" data-hash="0" hash="7">push_str</tspan>(<tspan data-hash="2">&amp;s2</tspan>); // shorthand for the above
<tspan fill="#AAA">6  </tspan>    <tspan class="fn" data-hash="0" hash="8">println!</tspan>("{}", <tspan data-hash="1">s1</tspan>); // prints "Hello, world, world"
<tspan fill="#AAA">7  </tspan>}
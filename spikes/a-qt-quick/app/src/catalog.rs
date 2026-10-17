//! Deterministic fake catalog: every Track is derived from a 32-bit seed so
//! that a million rows cost four bytes each.

const WORDS: &[&str] = &[
    "Midnight", "Pilot", "Ember", "Static", "Harbor", "Velvet", "Signal", "Drift", "Glass",
    "Hollow", "Neon", "Saturn", "Bloom", "Echo", "Atlas", "Copper", "Lantern", "Orbit", "Paper",
    "Quiet", "River", "Solace", "Tide", "Umber", "Vesper", "Wander", "Amber", "Cinder", "Dune",
    "Fable", "Gold", "Haze",
];
const ARTISTS: &[&str] = &[
    "Tycho", "Jon Hopkins", "Solar Fields", "RÜFÜS DU SOL", "Aphex Twin", "Bonobo",
    "Floating Points", "Four Tet", "Caribou", "Nils Frahm", "Ólafur Arnalds", "Kiasmos",
    "Rival Consoles", "Moderat", "Burial", "Jamie xx",
];
const MONTHS: &[&str] = &[
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

pub fn mix(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

pub fn title(seed: u32) -> String {
    let h = mix(seed);
    let a = WORDS[(h % WORDS.len() as u32) as usize];
    let b = WORDS[((h >> 8) % WORDS.len() as u32) as usize];
    if h & 0x30000 == 0 { format!("{a} {b} (Edit)") } else { format!("{a} {b}") }
}

pub fn artist(seed: u32) -> &'static str {
    ARTISTS[(mix(seed ^ 0x9e37) % ARTISTS.len() as u32) as usize]
}

pub fn album(seed: u32) -> String {
    let h = mix(seed ^ 0x51ed);
    format!("{} {}", WORDS[(h % 32) as usize], WORDS[((h >> 5) % 32) as usize])
}

pub fn duration(seed: u32) -> String {
    let s = 120 + mix(seed ^ 0x77) % 420;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn added(seed: u32) -> String {
    let h = mix(seed ^ 0x1234);
    format!("{} {}, {}", MONTHS[(h % 12) as usize], 1 + (h >> 4) % 28, 2016 + (h >> 9) % 10)
}

pub fn art(seed: u32) -> i32 {
    (mix(seed ^ 0xa11) % 512) as i32
}

/// Provider index into the Lumen source palette (green, orange, grey, blue).
pub fn source(seed: u32) -> i32 {
    (mix(seed ^ 0x5) % 4) as i32
}

pub fn liked(seed: u32) -> bool {
    mix(seed ^ 0x11) % 5 == 0
}

pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed | 1)
    }

    pub fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

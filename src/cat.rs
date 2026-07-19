/// The cat blinks every few ticks; ticks arrive ~8 times a second.
pub fn is_blinking(tick: u64) -> bool {
    tick % 40 < 3
}

/// The cat from cat.txt, for the home screen and the window header.
/// It cycles moods: watching with a flicking tail, enjoying a sushi
/// treat, and — only while music really plays — singing along with
/// dancing notes. Every row is padded to the same width so centered
/// art doesn't wobble between frames.
pub fn big_cat(tick: u64, media_playing: bool) -> [&'static str; 3] {
    if is_blinking(tick) {
        return [
            r" /\_/\      ",
            r"( -.- ) zZ  ",
            r" > ^ <      ",
        ];
    }
    match (tick / 24) % 6 {
        // a sushi treat
        0 => [
            r" /\_/\      ",
            r"( ^.^ )     ",
            r"  / >🍣     ",
        ],
        // singing along; the notes trade places to the beat
        1 | 2 if media_playing => {
            if tick % 8 < 4 {
                [
                    r" /\_/\   ♪  ",
                    r"( ^.^ ) ♫   ",
                    r" > ^ <      ",
                ]
            } else {
                [
                    r" /\_/\    ♫ ",
                    r"( ^.^ )  ♪  ",
                    r" > ^ <      ",
                ]
            }
        }
        // just watching, tail flicking
        _ => {
            if tick % 48 < 24 {
                [
                    r" /\_/\      ",
                    r"( o.o )     ",
                    r" > ^ < ~    ",
                ]
            } else {
                [
                    r" /\_/\      ",
                    r"( o.o )     ",
                    r" > ^ <   ~  ",
                ]
            }
        }
    }
}

/// True while the current frame is the singing one — the header uses
/// this to jiggle the cat's head in time.
pub fn is_singing(tick: u64, media_playing: bool) -> bool {
    media_playing && !is_blinking(tick) && matches!((tick / 24) % 6, 1 | 2)
}

/// Tiny cat that sits in the editor header, watching you.
/// Its eyes follow your cursor column.
pub fn small_cat(tick: u64, cursor_x: usize) -> &'static str {
    if is_blinking(tick) {
        "=(-ω-)="
    } else if cursor_x % 3 == 0 {
        "=(o.o)="
    } else if cursor_x % 3 == 1 {
        "=(o.-)="
    } else {
        "=(-.o)="
    }
}

/// The header cat with its head jiggling to the music: the same cat,
/// nudged left and right on the beat. Fixed width so nothing shifts.
pub fn jiggling_cat(tick: u64, cursor_x: usize) -> String {
    let head = small_cat(tick, cursor_x);
    if tick % 4 < 2 {
        format!("{head} ")
    } else {
        format!(" {head}")
    }
}

//! Where a dragged card settles. Like the Mac desktop: free placement, but near another card it lines up with it.
//! There's no grid across the whole screen, only alignment between neighbours and with the screen's edges.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn right(self) -> i32 {
        self.x + self.w
    }

    fn bottom(self) -> i32 {
        self.y + self.h
    }

    fn overlaps(self, other: Rect) -> bool {
        self.x < other.right() && other.x < self.right() && self.y < other.bottom() && other.y < self.bottom()
    }

    fn grown_by(self, amount: i32) -> Rect {
        Rect { x: self.x - amount, y: self.y - amount, w: self.w + amount * 2, h: self.h + amount * 2 }
    }

    pub fn holds(self, card: Rect) -> bool {
        card.x >= self.x && card.y >= self.y && card.right() <= self.right() && card.bottom() <= self.bottom()
    }
}

pub struct Rules {
    /// Space between neighbouring cards.
    pub gap: i32,
    /// Space kept from the screen's edges.
    pub margin: i32,
    /// How close a card has to come before it's pulled in.
    pub reach: i32,
}

/// The nearest place to `card` that's completely inside `area`. A card bigger than the area keeps its top left in it.
pub fn inside(card: Rect, area: Rect) -> Rect {
    Rect {
        x: card.x.min(area.right() - card.w).max(area.x),
        y: card.y.min(area.bottom() - card.h).max(area.y),
        ..card
    }
}

/// The position `moving` should settle at, which is where it already is when nothing is close enough. Icons aren't
/// lined up with, but a card never ends up on one, or on another card.
pub fn settle(moving: Rect, others: &[Rect], icons: &[Rect], area: Rect, rules: &Rules) -> (i32, i32) {
    let (x, y) = align(moving, others, icons, area, rules);
    keep_clear(Rect { x, y, ..moving }, others, icons, area, rules)
}

/// Where a card stays if it's clear of everything, or else the nearest place that is.
pub fn keep_clear(moving: Rect, others: &[Rect], icons: &[Rect], area: Rect, rules: &Rules) -> (i32, i32) {
    if !icons.iter().chain(others).any(|blocker| blocker.overlaps(moving)) {
        return (moving.x, moving.y);
    }
    nearest_clear(moving, others, icons, area, rules).unwrap_or((moving.x, moving.y))
}

fn nearest_clear(moving: Rect, others: &[Rect], icons: &[Rect], area: Rect, rules: &Rules) -> Option<(i32, i32)> {
    let mut xs = vec![moving.x];
    let mut ys = vec![moving.y];
    for blocker in icons.iter().chain(others) {
        xs.extend([blocker.right() + rules.gap, blocker.x - rules.gap - moving.w, blocker.right(), blocker.x - moving.w]);
        ys.extend([blocker.bottom() + rules.gap, blocker.y - rules.gap - moving.h, blocker.bottom(), blocker.y - moving.h]);
    }
    // Icons stand in rows and columns, so most of these repeat. Kept in order, so that ties go the same way. A place
    // that would put the card past the area's edge can never fit, so it isn't tried.
    let (xs, ys) = (unique(xs), unique(ys));
    let xs: Vec<i32> = xs.into_iter().filter(|x| *x >= area.x && *x + moving.w <= area.right()).collect();
    let ys: Vec<i32> = ys.into_iter().filter(|y| *y >= area.y && *y + moving.h <= area.bottom()).collect();
    let search = |space: i32| {
        // Only what reaches into the area can be in the way of a card inside it (icons on another screen can't).
        let grown = icons.iter().chain(others).map(|blocker| blocker.grown_by(space));
        let blockers = Cells::new(grown.filter(|blocker| blocker.overlaps(area)));
        // Each place's columns and rows worked out once, not again for every place it's paired with.
        let columns: Vec<_> = xs.iter().map(|x| Cells::span(&blockers.xs, *x, x + moving.w)).collect();
        let rows: Vec<_> = ys.iter().map(|y| Cells::span(&blockers.ys, *y, y + moving.h)).collect();
        nearest(&xs, &ys, moving, |x, y| !blockers.blocked(columns[x], rows[y]))
    };
    // A spot that keeps the usual distance wins over a nearer one that only just fits, as in free_spot. Nowhere that
    // only just fits means nowhere with the distance either, so a screen with no room is searched once, not twice.
    let tight = search(0)?;
    search(rules.gap).or(Some(tight))
}

// Every place, nearest first, the first that fits winning; of equally near ones, the one earlier in the lists. A
// row is left as soon as its places are farther than the best so far, and so is the search.
fn nearest(xs: &[i32], ys: &[i32], moving: Rect, fits: impl Fn(usize, usize) -> bool) -> Option<(i32, i32)> {
    let by_distance = |values: &[i32], from: i32| {
        let mut order: Vec<(i32, usize)> = values.iter().enumerate().map(|(index, value)| ((value - from).abs(), index)).collect();
        order.sort_unstable();
        order
    };
    let (across, down) = (by_distance(xs, moving.x), by_distance(ys, moving.y));
    let mut best: Option<((i32, usize, usize), (i32, i32))> = None;
    for &(dx, x) in &across {
        if best.is_some_and(|((distance, _, _), _)| dx > distance) {
            break;
        }
        for &(dy, y) in &down {
            let key = (dx + dy, x, y);
            if best.is_some_and(|(known, _)| key.0 > known.0) {
                break;
            }
            if best.is_some_and(|(known, _)| key >= known) {
                continue;
            }
            if fits(x, y) {
                best = Some((key, (xs[x], ys[y])));
                break; // The rest of this row is farther, or as near but later in the list.
            }
        }
    }
    best.map(|(_, position)| position)
}

// The blockers on a grid cut along their own edges, so each cell is either all blocked or all free, and a table of
// running totals says at once whether any rectangle holds a blocked cell. Exact, and the same cost for every place.
struct Cells {
    xs: Vec<i32>,
    ys: Vec<i32>,
    totals: Vec<u32>,
}

impl Cells {
    fn new(blockers: impl Iterator<Item = Rect>) -> Self {
        let blockers: Vec<Rect> = blockers.collect();
        let edges = |pick: fn(&Rect) -> [i32; 2]| {
            let mut edges: Vec<i32> = blockers.iter().flat_map(pick).collect();
            edges.sort_unstable();
            edges.dedup();
            edges
        };
        let (xs, ys) = (edges(|b| [b.x, b.right()]), edges(|b| [b.y, b.bottom()]));
        let (columns, rows) = (xs.len().saturating_sub(1), ys.len().saturating_sub(1));
        let mut totals = vec![0u32; (columns + 1) * (rows + 1)];
        let at = |column: usize, row: usize| row * (columns + 1) + column;
        for blocker in &blockers {
            let (left, right) = (xs.partition_point(|x| *x < blocker.x), xs.partition_point(|x| *x < blocker.right()));
            let (top, bottom) = (ys.partition_point(|y| *y < blocker.y), ys.partition_point(|y| *y < blocker.bottom()));
            for row in top..bottom {
                for column in left..right {
                    totals[at(column + 1, row + 1)] = 1;
                }
            }
        }
        for row in 1..=rows {
            for column in 1..=columns {
                totals[at(column, row)] += totals[at(column - 1, row)] + totals[at(column, row - 1)] - totals[at(column - 1, row - 1)];
            }
        }
        Self { xs, ys, totals }
    }

    // The cells between two edges a rectangle reaches into: from the one holding `from` to before `to`.
    fn span(edges: &[i32], from: i32, to: i32) -> (usize, usize) {
        let cells = edges.len().saturating_sub(1);
        (edges.partition_point(|edge| *edge <= from).saturating_sub(1), edges.partition_point(|edge| *edge < to).min(cells))
    }

    #[cfg(test)]
    fn overlaps(&self, rect: Rect) -> bool {
        self.blocked(Self::span(&self.xs, rect.x, rect.right()), Self::span(&self.ys, rect.y, rect.bottom()))
    }

    // Whether any of these cells (from span) is blocked.
    fn blocked(&self, (left, right): (usize, usize), (top, bottom): (usize, usize)) -> bool {
        if left >= right || top >= bottom {
            return false;
        }
        let columns = self.xs.len();
        let at = |column: usize, row: usize| row * columns + column;
        self.totals[at(right, bottom)] + self.totals[at(left, top)] > self.totals[at(left, bottom)] + self.totals[at(right, top)]
    }
}

fn unique(mut values: Vec<i32>) -> Vec<i32> {
    let mut seen = std::collections::HashSet::new();
    values.retain(|value| seen.insert(*value));
    values
}

fn align(moving: Rect, others: &[Rect], icons: &[Rect], area: Rect, rules: &Rules) -> (i32, i32) {
    let mut xs = vec![moving.x];
    let mut ys = vec![moving.y];
    let near = rules.gap + rules.reach;
    for other in others {
        // Lining up only makes sense with a card that's next to this one on the other axis.
        if moving.y < other.bottom() + near && other.y < moving.bottom() + near {
            xs.extend([other.x, other.right() - moving.w, other.right() + rules.gap, other.x - rules.gap - moving.w]);
        }
        if moving.x < other.right() + near && other.x < moving.right() + near {
            ys.extend([other.y, other.bottom() - moving.h, other.bottom() + rules.gap, other.y - rules.gap - moving.h]);
        }
    }
    xs.extend([area.x + rules.margin, area.right() - rules.margin - moving.w]);
    ys.extend([area.y + rules.margin, area.bottom() - rules.margin - moving.h]);
    // Nothing past the screen's edge pulls a card in, and staying put is always an option.
    xs.retain(|x| *x == moving.x || ((x - moving.x).abs() <= rules.reach && *x >= area.x && *x + moving.w <= area.right()));
    ys.retain(|y| *y == moving.y || ((y - moving.y).abs() <= rules.reach && *y >= area.y && *y + moving.h <= area.bottom()));

    let mut best = None;
    for &x in &xs {
        for &y in &ys {
            let placed = Rect { x, y, ..moving };
            if others.iter().chain(icons).any(|other| placed.overlaps(*other)) {
                continue;
            }
            // Prefer really aligning on both axes over staying put on one.
            let free = i32::from(x == moving.x) + i32::from(y == moving.y);
            let cost = (free, (x - moving.x).abs() + (y - moving.y).abs());
            if best.is_none_or(|(known, _)| cost < known) {
                best = Some((cost, (x, y)));
            }
        }
    }
    best.map_or((moving.x, moving.y), |(_, position)| position)
}

/// Where a resized card goes: it keeps a corner of where it was, whichever one leaves it clear of `blockers`.
/// Trying the top-left first means a card normally grows right and down. A spot that keeps the usual distances wins
/// over one that only just fits. None if every corner is blocked.
pub fn free_spot(old: Rect, width: i32, height: i32, blockers: &[Rect], area: Rect, rules: &Rules) -> Option<Rect> {
    let (left, top) = (old.x, old.y);
    let (right, bottom) = (old.right() - width, old.bottom() - height);
    // A card that only gets smaller has no reason to go anywhere, however close its neighbours are.
    let stays = Rect { x: left, y: top, w: width, h: height };
    if width <= old.w && height <= old.h && area.holds(stays) && !blockers.iter().any(|other| stays.overlaps(*other)) {
        return Some(stays);
    }
    let corners = [(left, top), (right, top), (left, bottom), (right, bottom)];
    let roomy = area.grown_by(-rules.margin);
    [(roomy, rules.gap), (area, 0)].into_iter().find_map(|(within, distance)| {
        corners.into_iter().find_map(|(x, y)| {
            let card = Rect { x, y, w: width, h: height };
            let wider = card.grown_by(distance);
            (within.holds(card) && !blockers.iter().any(|other| wider.overlaps(*other))).then_some(card)
        })
    })
}

/// Where a resized card goes when no corner is free: on the screen, keeping its distance from the edges.
pub fn squeezed(card: Rect, area: Rect, rules: &Rules) -> Rect {
    inside(card, area.grown_by(-rules.margin))
}

/// Moves whatever a card that just grew now covers to the nearest free spot, on the screen and off the icons. Cards
/// it doesn't cover stay exactly where they are.
pub fn make_room(cards: &mut [Rect], grown: usize, icons: &[Rect], area: Rect, rules: &Rules) {
    let mut settled = vec![cards[grown]];
    let mut order: Vec<usize> = (0..cards.len()).filter(|index| *index != grown).collect();
    order.sort_by_key(|index| (cards[*index].y, cards[*index].x));
    for index in order {
        if settled.iter().any(|other| other.overlaps(cards[index])) {
            let (x, y) = nearest_clear(cards[index], &settled, icons, area, rules).unwrap_or((cards[index].x, cards[index].y));
            (cards[index].x, cards[index].y) = (x, y);
        }
        settled.push(cards[index]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: Rules = Rules { gap: 22, margin: 24, reach: 30 };
    const SCREEN: Rect = Rect { x: 0, y: 0, w: 1440, h: 900 };

    fn card(x: i32, y: i32) -> Rect {
        Rect { x, y, w: 158, h: 158 }
    }

    fn icon(x: i32, y: i32) -> Rect {
        Rect { x, y, w: 80, h: 90 }
    }

    #[test]
    fn far_from_everything_stays_put() {
        assert_eq!(settle(card(500, 400), &[card(1000, 100)], &[], SCREEN, &RULES), (500, 400));
    }

    #[test]
    fn slides_off_another_card() {
        let wide = Rect { x: 600, y: 300, w: 338, h: 338 };
        let (x, y) = settle(card(700, 400), &[wide], &[], SCREEN, &RULES);
        assert!(!card(x, y).overlaps(wide));
    }

    #[test]
    fn a_large_card_dropped_across_a_column_clears_all_of_it() {
        let column = [card(800, 100), Rect { x: 800, y: 280, w: 338, h: 158 }, card(800, 460), card(980, 460)];
        let moving = Rect { x: 500, y: 200, w: 338, h: 338 };
        let (x, y) = settle(moving, &column, &[], SCREEN, &RULES);
        let placed = Rect { x, y, ..moving };
        assert!(!column.iter().any(|other| other.overlaps(placed)));
    }

    #[test]
    fn keeping_clear_leaves_a_free_card_alone() {
        assert_eq!(keep_clear(card(100, 100), &[card(600, 100)], &[], SCREEN, &RULES), (100, 100));
    }

    #[test]
    fn slides_off_the_icons() {
        let icons = [icon(0, 0), icon(0, 100), icon(0, 200), icon(0, 300)];
        let (x, y) = settle(card(40, 120), &[], &icons, SCREEN, &RULES);
        assert_eq!((x, y), (80 + 22, 120));
    }

    #[test]
    fn a_card_clear_of_the_icons_stays_put() {
        let icons = [icon(0, 0), icon(0, 100)];
        assert_eq!(settle(card(300, 300), &[], &icons, SCREEN, &RULES), (300, 300));
    }

    #[test]
    fn slides_off_icons_without_landing_on_a_card() {
        let icons = [icon(0, 0), icon(0, 100), icon(0, 200)];
        let others = [card(102, 100)];
        let placed = settle(card(40, 120), &others, &icons, SCREEN, &RULES);
        let settled = Rect { x: placed.0, y: placed.1, w: 158, h: 158 };
        assert!(!others.iter().chain(&icons).any(|blocker| blocker.overlaps(settled)));
    }

    #[test]
    fn lands_beside_a_neighbour_with_the_gap() {
        let other = card(300, 300);
        assert_eq!(settle(card(300 + 158 + 30, 310), &[other], &[], SCREEN, &RULES), (300 + 158 + 22, 300));
    }

    #[test]
    fn lands_below_a_neighbour_with_the_gap() {
        let other = card(300, 300);
        assert_eq!(settle(card(312, 300 + 158 + 15), &[other], &[], SCREEN, &RULES), (300, 300 + 158 + 22));
    }

    #[test]
    fn wide_cards_align_on_their_right_edges() {
        let wide = Rect { x: 300, y: 300, w: 338, h: 158 };
        let settled = settle(card(300 + 338 - 158 + 10, 300 + 158 + 20), &[wide], &[], SCREEN, &RULES);
        assert_eq!(settled, (300 + 338 - 158, 300 + 158 + 22));
    }

    #[test]
    fn keeps_clear_of_the_screen_edges() {
        assert_eq!(settle(card(10, 15), &[], &[], SCREEN, &RULES), (24, 24));
        assert_eq!(settle(card(1440 - 158 - 10, 300), &[], &[], SCREEN, &RULES).0, 1440 - 158 - 24);
    }

    #[test]
    fn grows_right_and_down_when_there_is_room() {
        let spot = free_spot(card(300, 300), 338, 338, &[], SCREEN, &RULES).unwrap();
        assert_eq!((spot.x, spot.y), (300, 300));
    }

    #[test]
    fn grows_away_from_a_neighbour() {
        let neighbour = card(300 + 158 + 22, 300);
        let spot = free_spot(card(300, 300), 338, 158, &[neighbour], SCREEN, &RULES).unwrap();
        assert_eq!((spot.x, spot.y), (300 + 158 - 338, 300));
    }

    #[test]
    fn grows_up_at_the_bottom_of_the_screen() {
        let spot = free_spot(card(300, 742), 338, 338, &[], SCREEN, &RULES).unwrap();
        assert_eq!((spot.x, spot.y), (300, 742 + 158 - 338));
    }

    #[test]
    fn says_so_when_every_corner_is_blocked() {
        let around = [card(100, 300), card(480, 300), card(300, 120), card(300, 480)];
        assert_eq!(free_spot(card(300, 300), 338, 338, &around, SCREEN, &RULES), None);
    }

    #[test]
    fn growing_pushes_the_card_below_down() {
        let mut cards = [Rect { x: 300, y: 100, w: 338, h: 338 }, card(300, 280)];
        make_room(&mut cards, 0, &[], SCREEN, &RULES);
        assert_eq!((cards[1].x, cards[1].y), (300, 100 + 338 + 22));
    }

    #[test]
    fn pushing_carries_on_down_the_column() {
        let mut cards = [Rect { x: 300, y: 0, w: 338, h: 338 }, card(300, 180), card(300, 360)];
        make_room(&mut cards, 0, &[], SCREEN, &RULES);
        assert_eq!(cards[1].y, 360);
        assert_eq!(cards[2].y, 360 + 158 + 22);
    }

    #[test]
    fn out_of_screen_goes_to_the_next_column() {
        let mut cards = [Rect { x: 1000, y: 500, w: 338, h: 338 }, card(1000, 700)];
        make_room(&mut cards, 0, &[], SCREEN, &RULES);
        assert!(SCREEN.holds(cards[1]) && !cards[1].overlaps(cards[0]));
    }

    #[test]
    fn a_pushed_card_stays_on_the_screen() {
        // Used to end up at x = -180, out of sight, with room to the right.
        let mut cards = [Rect { x: 0, y: 500, w: 338, h: 338 }, card(0, 320), card(0, 700)];
        make_room(&mut cards, 0, &[], SCREEN, &RULES);
        for pushed in &cards[1..] {
            assert!(SCREEN.holds(*pushed), "{pushed:?}");
            assert!(!pushed.overlaps(cards[0]));
        }
    }

    #[test]
    fn a_pushed_card_stays_off_the_icons() {
        let icons = [icon(300, 470), icon(390, 470), icon(480, 470), icon(570, 470)];
        let mut cards = [Rect { x: 300, y: 100, w: 338, h: 338 }, card(300, 280)];
        make_room(&mut cards, 0, &icons, SCREEN, &RULES);
        assert!(!icons.iter().any(|icon| icon.overlaps(cards[1])));
        assert!(!cards[0].overlaps(cards[1]));
    }

    #[test]
    fn cards_nobody_covers_are_left_alone_even_on_an_icon() {
        let icons = [icon(900, 100)];
        let before = [card(100, 100), card(880, 100)];
        let mut cards = before;
        make_room(&mut cards, 0, &icons, SCREEN, &RULES);
        assert_eq!(cards, before);
    }

    #[test]
    fn growing_keeps_the_gap_when_it_can() {
        // The top left would fit, but touching the neighbour: the other corner keeps the distance.
        let neighbour = card(300 + 338 + 5, 300);
        let spot = free_spot(card(300, 300), 338, 158, &[neighbour], SCREEN, &RULES).unwrap();
        assert_eq!(spot.x, 300 + 158 - 338);
    }

    #[test]
    fn growing_falls_back_to_just_fitting() {
        let neighbours = [card(300 + 338 + 5, 300), card(300 - 180 - 158 - 5, 300)];
        let spot = free_spot(card(300, 300), 338, 158, &neighbours, SCREEN, &RULES).unwrap();
        assert_eq!((spot.x, spot.y), (300, 300));
    }

    #[test]
    fn a_squeezed_card_keeps_off_the_edge() {
        let spot = squeezed(Rect { x: 1300, y: 800, w: 338, h: 158 }, SCREEN, &RULES);
        assert_eq!((spot.x, spot.y), (1440 - 24 - 338, 900 - 24 - 158));
    }

    #[test]
    fn inside_pulls_a_card_back_on_every_side() {
        assert_eq!(inside(card(-50, 100), SCREEN).x, 0);
        assert_eq!(inside(card(1400, 100), SCREEN).x, 1440 - 158);
        assert_eq!(inside(card(100, -50), SCREEN).y, 0);
        assert_eq!(inside(card(100, 850), SCREEN).y, 900 - 158);
        assert_eq!(inside(card(100, 100), SCREEN), card(100, 100));
    }

    #[test]
    fn inside_keeps_the_top_left_of_a_card_too_big_for_the_area() {
        let huge = Rect { x: 50, y: 50, w: 2000, h: 2000 };
        assert_eq!(inside(huge, SCREEN), Rect { x: 0, y: 0, ..huge });
    }

    #[test]
    fn nothing_past_the_edge_pulls_a_card_in() {
        // Beside the neighbour would be past the right edge of the screen.
        let neighbour = Rect { x: 1100, y: 300, w: 338, h: 158 };
        let moving = inside(card(1282, 500), SCREEN);
        let (x, _) = settle(moving, &[neighbour], &[], SCREEN, &RULES);
        assert!(x + 158 <= 1440);
    }

    #[test]
    fn a_pushed_card_keeps_the_gap_from_the_one_that_grew() {
        // An icon far away on the other side used to offer a spot touching the grown card.
        let icons = [icon(0, 100 + 338 + 22 + 158)];
        let mut cards = [Rect { x: 900, y: 100, w: 338, h: 338 }, card(900, 280)];
        make_room(&mut cards, 0, &icons, SCREEN, &RULES);
        assert_eq!((cards[1].x, cards[1].y), (900, 100 + 338 + 22));
    }

    #[test]
    fn a_spot_too_tight_for_the_gap_still_beats_overlapping() {
        // The only hole left is wider than the card but not by the gap on both sides.
        let left = Rect { x: 0, y: 0, w: 600, h: 900 };
        let right = Rect { x: 770, y: 0, w: 670, h: 900 };
        let (x, y) = keep_clear(card(590, 100), &[left, right], &[], SCREEN, &RULES);
        let placed = card(x, y);
        assert!(!placed.overlaps(left) && !placed.overlaps(right), "{placed:?}");
    }

    #[test]
    fn a_card_getting_smaller_stays_where_it_is() {
        // Closer to the neighbour than the gap, which is no reason to change corners.
        let neighbour = card(300 + 338 + 5, 300);
        let spot = free_spot(Rect { x: 300, y: 300, w: 338, h: 338 }, 158, 158, &[neighbour], SCREEN, &RULES).unwrap();
        assert_eq!((spot.x, spot.y), (300, 300));
    }

    #[test]
    fn a_card_at_the_top_edge_stays_there_getting_smaller() {
        let spot = free_spot(Rect { x: 300, y: 0, w: 338, h: 338 }, 338, 158, &[], SCREEN, &RULES).unwrap();
        assert_eq!((spot.x, spot.y), (300, 0));
    }

    #[test]
    fn hundreds_of_icons_are_searched_quickly() {
        let icons: Vec<Rect> = (0..400).map(|n| icon((n % 25) * 80, (n / 25) * 90)).collect();
        let screen = Rect { x: 0, y: 0, w: 2880, h: 1800 };
        let moving = Rect { x: 40, y: 40, w: 338, h: 338 };
        let started = std::time::Instant::now();
        let (x, y) = keep_clear(moving, &[], &icons, screen, &RULES);
        let placed = Rect { x, y, ..moving };
        assert!(!icons.iter().any(|icon| icon.overlaps(placed)) && screen.holds(placed), "{placed:?}");
        assert!(started.elapsed() < std::time::Duration::from_millis(500), "{:?}", started.elapsed());
    }

    // The search as it was before: every place, nearest wins, earlier in the lists on a tie.
    fn every_place(moving: Rect, others: &[Rect], icons: &[Rect], area: Rect, rules: &Rules) -> Option<(i32, i32)> {
        let mut xs = vec![moving.x];
        let mut ys = vec![moving.y];
        for blocker in icons.iter().chain(others) {
            xs.extend([blocker.right() + rules.gap, blocker.x - rules.gap - moving.w, blocker.right(), blocker.x - moving.w]);
            ys.extend([blocker.bottom() + rules.gap, blocker.y - rules.gap - moving.h, blocker.bottom(), blocker.y - moving.h]);
        }
        let (xs, ys) = (unique(xs), unique(ys));
        [rules.gap, 0].into_iter().find_map(|space| {
            let mut best = None;
            for &x in &xs {
                for &y in &ys {
                    let card = Rect { x, y, ..moving };
                    let distance = (x - moving.x).abs() + (y - moving.y).abs();
                    let clear = area.holds(card) && !icons.iter().chain(others).any(|blocker| blocker.overlaps(card.grown_by(space)));
                    if clear && best.is_none_or(|(known, _)| distance < known) {
                        best = Some((distance, (x, y)));
                    }
                }
            }
            best.map(|(_, position)| position)
        })
    }

    #[test]
    fn the_quick_search_lands_where_trying_every_place_does() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = |below: i32| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % below as u64) as i32
        };
        for _ in 0..3000 {
            let icons: Vec<Rect> = (0..next(30)).map(|_| icon(next(1360), next(810))).collect();
            let others: Vec<Rect> = (0..next(5)).map(|_| card(next(1280), next(740))).collect();
            let moving = Rect { x: next(1300), y: next(760), w: [158, 338][next(2) as usize], h: [158, 338][next(2) as usize] };
            let (quick, every) = (nearest_clear(moving, &others, &icons, SCREEN, &RULES), every_place(moving, &others, &icons, SCREEN, &RULES));
            assert_eq!(quick, every, "{moving:?}");
        }
    }

    #[test]
    fn hundreds_of_scattered_icons_are_searched_quickly() {
        // Not lined up, so hardly any place repeats: the case that used to take most of a second.
        let screen = Rect { x: 0, y: 0, w: 2880, h: 1800 };
        let mut icons = Vec::new();
        let mut seed = 12345u64;
        while icons.len() < 400 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let candidate = icon((seed >> 33) as i32 % 2800, (seed >> 13) as i32 % 1710);
            if !icons.iter().any(|placed: &Rect| placed.overlaps(candidate)) {
                icons.push(candidate);
            }
        }
        let moving = Rect { x: 1200, y: 700, w: 338, h: 338 };
        let started = std::time::Instant::now();
        let (x, y) = keep_clear(moving, &[], &icons, screen, &RULES);
        let placed = Rect { x, y, ..moving };
        // The program is an optimised build; a debug build of the tests is several times slower.
        let limit = if cfg!(debug_assertions) { 2000 } else { 200 };
        assert!(started.elapsed() < std::time::Duration::from_millis(limit), "{:?}", started.elapsed());
        assert!((x, y) == (moving.x, moving.y) || !icons.iter().any(|icon| icon.overlaps(placed)));
    }

    // Icons scattered over `screens` until no big card fits anywhere: every drag update searches everything.
    fn no_room_for_a_big_card(screens: &[Rect], count: usize) -> Vec<Rect> {
        let mut icons = Vec::new();
        let mut seed = 4242u64;
        while icons.len() < count {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let screen = screens[(seed >> 60) as usize % screens.len()];
            let candidate = Rect { x: screen.x + (seed >> 33) as i32 % (screen.w - 80), y: screen.y + (seed >> 13) as i32 % (screen.h - 90), w: 80, h: 90 };
            if !icons.iter().any(|placed: &Rect| placed.overlaps(candidate)) {
                icons.push(candidate);
            }
        }
        icons
    }

    #[test]
    fn a_drag_with_no_room_anywhere_stays_quick() {
        // A large card at 200% (338 x 2 x 85%), dragged over icons that leave it nowhere to go.
        let rules = Rules { gap: 37, margin: 41, reach: 51 };
        let one = Rect { x: 0, y: 0, w: 2880, h: 2160 };
        let (left, right) = (Rect { x: 0, y: 0, w: 3840, h: 2160 }, Rect { x: 3840, y: 0, w: 3840, h: 2160 });
        for (screens, count, area) in [(vec![one], 400, one), (vec![left, right], 1000, left)] {
            let icons = no_room_for_a_big_card(&screens, count);
            // Checked once, slowly: there really is no room, so each update below searches everything.
            assert_eq!(every_place(Rect { x: 900, y: 700, w: 575, h: 575 }, &[], &icons, area, &rules), None);
            let mut times = Vec::new();
            for step in 0..10 {
                let moving = Rect { x: 900 + step * 7, y: 700 + step * 5, w: 575, h: 575 };
                let started = std::time::Instant::now();
                let (x, y) = settle(moving, &[], &icons, area, &rules);
                times.push(started.elapsed());
                assert_eq!((x, y), (moving.x, moving.y), "nowhere to go: the card stays where it is");
            }
            times.sort();
            eprintln!("{count} icons: median {:?}", times[5]);
            let limit = if cfg!(debug_assertions) { 200 } else { 16 };
            assert!(times[5] < std::time::Duration::from_millis(limit), "{count} icons: {:?}", times[5]);
        }
    }

    #[test]
    fn a_crowded_screen_with_room_left_never_ends_on_an_icon() {
        // Enough scattered icons that the exact search gives up before reaching the free spot.
        let screen = Rect { x: 0, y: 0, w: 2880, h: 1800 };
        let mut seed = 777u64;
        for round in 0..20 {
            let mut icons = Vec::new();
            while icons.len() < 120 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let candidate = icon((seed >> 33) as i32 % 2800, (seed >> 13) as i32 % 1710);
                if !icons.iter().any(|placed: &Rect| placed.grown_by(30).overlaps(candidate)) {
                    icons.push(candidate);
                }
            }
            let moving = Rect { x: 1200, y: 700, w: 676, h: 676 };
            let (x, y) = keep_clear(moving, &[], &icons, screen, &RULES);
            let placed = Rect { x, y, ..moving };
            let room = every_place(moving, &[], &icons, screen, &RULES).is_some();
            assert!(!room || !icons.iter().any(|icon| icon.overlaps(placed)), "round {round}: {placed:?}");
        }
    }

    #[test]
    fn a_card_fits_a_channel_just_wider_than_it() {
        // 136 pixels between two columns of icons, for a card 134 wide: no room for the gap, but room.
        let icons: Vec<Rect> = (0..10).flat_map(|row| [Rect { x: 500, y: row * 90, w: 80, h: 90 }, Rect { x: 716, y: row * 90, w: 80, h: 90 }]).collect();
        let moving = Rect { x: 560, y: 300, w: 134, h: 134 };
        let (x, y) = keep_clear(moving, &[], &icons, Rect { x: 0, y: 0, w: 1300, h: 900 }, &RULES);
        let placed = Rect { x, y, ..moving };
        assert!(!icons.iter().any(|icon| icon.overlaps(placed)), "{placed:?}");
    }

    #[test]
    fn the_cells_agree_with_checking_every_blocker() {
        let mut seed = 99u64;
        let mut next = |below: i32| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) % below as u64) as i32
        };
        for _ in 0..2000 {
            let blockers: Vec<Rect> = (0..next(40)).map(|_| Rect { x: next(1400), y: next(880), w: 1 + next(300), h: 1 + next(300) }).collect();
            let cells = Cells::new(blockers.iter().copied());
            for _ in 0..50 {
                let rect = Rect { x: next(1500) - 50, y: next(950) - 50, w: 1 + next(400), h: 1 + next(400) };
                assert_eq!(cells.overlaps(rect), blockers.iter().any(|b| b.overlaps(rect)), "{rect:?}");
            }
        }
    }

    #[test]
    fn nothing_moves_without_an_overlap() {
        let before = [card(100, 100), card(400, 100)];
        let mut cards = before;
        make_room(&mut cards, 0, &[], SCREEN, &RULES);
        assert_eq!(cards, before);
    }

    #[test]
    fn never_settles_on_top_of_another_card() {
        let others = [card(300, 300), card(300, 480)];
        let (x, y) = settle(card(310, 310), &others, &[], SCREEN, &RULES);
        let placed = card(x, y);
        assert!(!others.iter().any(|other| placed.overlaps(*other)) || (x, y) == (310, 310));
    }
}

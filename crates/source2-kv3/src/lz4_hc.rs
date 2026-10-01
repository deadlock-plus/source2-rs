//! LZ4 high-compression block encoder.
//!
//! `lz4_flex` has no high-compression mode, and Valve's KV3 files come from one: every LZ4
//! buffer and blob chunk checked re-encodes to the same bytes at the reference encoder's level
//! 12. This is a port of that reference algorithm - price-based parsing over hash chains for
//! [`Level::Optimal`], which is level 12, and the cheaper lazy matching for [`Level::Chain`],
//! level 9, which nothing in the crate uses outside its tests. Long single-byte runs are not
//! special-cased the way the reference does, so they are searched in full.

const MIN_MATCH: usize = 4;
const MF_LIMIT: usize = 12;
const LAST_LITERALS: usize = 5;
const MIN_LENGTH: usize = MF_LIMIT + 1;
const MAX_DISTANCE: usize = 65_535;
const HASH_LOG: u32 = 15;
const RUN_MASK: usize = 15;
const ML_MASK: usize = 15;
/// Longest match the lazy parser will keep extending a first match to before looking ahead.
const OPTIMAL_ML: usize = (ML_MASK - 1) + MIN_MATCH;
const OPT_NUM: usize = 1 << 12;
const TRAILING_LITERALS: usize = 3;

/// How hard to search. The two presets mirror the reference encoder's levels 9 and 12.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum Level {
    /// Lazy matching over hash chains.
    Chain { attempts: usize },
    /// Price-based parsing over the same chains.
    Optimal {
        attempts: usize,
        sufficient: usize,
        full_update: bool,
    },
}

impl Level {
    #[cfg(test)]
    pub(crate) const L9: Level = Level::Chain { attempts: 1024 };
    pub(crate) const L12: Level = Level::Optimal {
        attempts: 16384,
        sufficient: OPT_NUM,
        full_update: true,
    };
}

/// Compress `input` into one LZ4 block that may reference the `window` before it.
pub(crate) fn compress(window: &[u8], input: &[u8], level: Level) -> Vec<u8> {
    let mut data = Vec::with_capacity(window.len() + input.len());
    data.extend_from_slice(window);
    data.extend_from_slice(input);
    let mut enc = Encoder {
        data: &data,
        head: vec![0; 1 << HASH_LOG],
        chain: vec![0; data.len()],
        next: 0,
        anchor: window.len(),
        out: Vec::new(),
    };
    if input.len() >= MIN_LENGTH {
        match level {
            Level::Chain { attempts } => enc.chain_parse(window.len(), attempts),
            Level::Optimal {
                attempts,
                sufficient,
                full_update,
            } => enc.optimal_parse(window.len(), attempts, sufficient, full_update),
        }
    }
    enc.finish()
}

struct Found {
    len: usize,
    /// Index of the match source's first byte.
    pos: usize,
    /// Index the match starts at in the input, which can be before the position searched.
    start: usize,
}

struct Encoder<'a> {
    data: &'a [u8],
    /// Newest position for each hash, plus one; zero means empty.
    head: Vec<u32>,
    /// Distance to the previous position with the same hash, capped at the window size.
    chain: Vec<u16>,
    /// First position not yet inserted into the chains.
    next: usize,
    anchor: usize,
    out: Vec<u8>,
}

fn hash(data: &[u8], at: usize) -> usize {
    let v = u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
    (v.wrapping_mul(2_654_435_761) >> (32 - HASH_LOG)) as usize
}

fn read16_eq(data: &[u8], a: usize, b: usize) -> bool {
    match (data.get(a..a + 2), data.get(b..b + 2)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

fn literals_price(literals: usize) -> isize {
    let mut price = literals;
    if literals >= RUN_MASK {
        price += 1 + (literals - RUN_MASK) / 255;
    }
    price as isize
}

fn sequence_price(literals: usize, match_len: usize) -> isize {
    let mut price = 1 + 2 + literals_price(literals);
    if match_len >= ML_MASK + MIN_MATCH {
        price += 1 + ((match_len - (ML_MASK + MIN_MATCH)) / 255) as isize;
    }
    price
}

fn push_extension(out: &mut Vec<u8>, mut n: usize) {
    while n >= 255 {
        out.push(255);
        n -= 255;
    }
    out.push(n as u8);
}

impl Encoder<'_> {
    fn insert(&mut self, target: usize) {
        while self.next < target {
            let i = self.next;
            if i + 4 <= self.data.len() {
                let h = hash(self.data, i);
                let previous = self.head[h] as usize;
                let delta = if previous == 0 {
                    MAX_DISTANCE
                } else {
                    (i - (previous - 1)).min(MAX_DISTANCE)
                };
                self.chain[i] = delta as u16;
                self.head[h] = (i + 1) as u32;
            }
            self.next += 1;
        }
    }

    fn count(&self, mut a: usize, mut b: usize, limit: usize) -> usize {
        let start = a;
        while a < limit && self.data[a] == self.data[b] {
            a += 1;
            b += 1;
        }
        a - start
    }

    fn count_back(&self, mut ip: usize, mut source: usize, low: usize) -> usize {
        let start = ip;
        while ip > low && source > 0 && self.data[ip - 1] == self.data[source - 1] {
            ip -= 1;
            source -= 1;
        }
        start - ip
    }

    /// The longest match for `ip` that beats `longest`, searching up to `attempts` chain
    /// links. A match may be extended backwards as far as `low`.
    fn find(
        &mut self,
        ip: usize,
        low: usize,
        high: usize,
        mut longest: usize,
        attempts: usize,
        chain_swap: bool,
    ) -> Option<Found> {
        self.insert(ip);
        let look_back = ip - low;
        let lowest = ip.saturating_sub(MAX_DISTANCE);
        let pattern = &self.data[ip..ip + 4];
        let mut found = None;
        let mut chain_pos = 0;
        let mut left = attempts;
        let mut candidate = self.head[hash(self.data, ip)] as usize;
        while candidate != 0 && left > 0 {
            let at = candidate - 1;
            if at < lowest {
                break;
            }
            if at >= ip {
                match self.step(at, chain_pos) {
                    Some(n) => candidate = n,
                    None => break,
                }
                continue;
            }
            left -= 1;
            let mut len = 0;
            let probe = longest - 1;
            let source = (at + probe).checked_sub(look_back);
            if source.is_some_and(|s| read16_eq(self.data, low + probe, s))
                && &self.data[at..at + 4] == pattern
            {
                let back = if look_back > 0 {
                    self.count_back(ip, at, low)
                } else {
                    0
                };
                len = MIN_MATCH + self.count(ip + MIN_MATCH, at + MIN_MATCH, high) + back;
                if len > longest {
                    longest = len;
                    found = Some(Found {
                        len,
                        pos: at - back,
                        start: ip - back,
                    });
                }
            }
            if chain_swap && len == longest && at + longest <= ip {
                let end = longest - MIN_MATCH + 1;
                let mut distance_to_next = 1;
                let mut accel = 1 << 4;
                let mut pos = 0;
                while pos < end {
                    let d = usize::from(self.chain[at + pos]);
                    let step = accel >> 4;
                    accel += 1;
                    if d > distance_to_next {
                        distance_to_next = d;
                        chain_pos = pos;
                        accel = 1 << 4;
                    }
                    pos += step;
                }
                if distance_to_next > 1 {
                    if distance_to_next > at {
                        break;
                    }
                    candidate = at - distance_to_next + 1;
                    continue;
                }
            }
            match self.step(at, chain_pos) {
                Some(n) => candidate = n,
                None => break,
            }
        }
        found
    }

    /// The next candidate down the chain from `at`, as position plus one.
    fn step(&self, at: usize, chain_pos: usize) -> Option<usize> {
        let delta = usize::from(*self.chain.get(at + chain_pos)?);
        if delta == 0 || delta > at {
            return None;
        }
        Some(at - delta + 1)
    }

    fn emit(&mut self, ip: usize, match_len: usize, source: usize) {
        let literals = &self.data[self.anchor..ip];
        let ml = match_len - MIN_MATCH;
        let out = &mut self.out;
        out.push((literals.len().min(RUN_MASK) as u8) << 4 | ml.min(ML_MASK) as u8);
        if literals.len() >= RUN_MASK {
            push_extension(out, literals.len() - RUN_MASK);
        }
        out.extend_from_slice(literals);
        out.extend_from_slice(&((ip - source) as u16).to_le_bytes());
        if ml >= ML_MASK {
            push_extension(out, ml - ML_MASK);
        }
        self.anchor = ip + match_len;
    }

    fn finish(mut self) -> Vec<u8> {
        let literals = &self.data[self.anchor..];
        self.out.push((literals.len().min(RUN_MASK) as u8) << 4);
        if literals.len() >= RUN_MASK {
            push_extension(&mut self.out, literals.len() - RUN_MASK);
        }
        self.out.extend_from_slice(literals);
        self.out
    }

    /// The best match at `ip` that is longer than `min_len`.
    fn longer(
        &mut self,
        ip: usize,
        limit: usize,
        min_len: usize,
        attempts: usize,
    ) -> Option<Longer> {
        let found = self.find(ip, ip, limit, min_len, attempts, true)?;
        Some(Longer {
            len: found.len,
            off: ip - found.pos,
        })
    }

    /// Lazy matching: a match is kept only if looking ahead for one or two better ones
    /// finds nothing that overlaps it to advantage.
    fn chain_parse(&mut self, start: usize, attempts: usize) {
        let end = self.data.len();
        let mf_limit = end - MF_LIMIT;
        let limit = end - LAST_LITERALS;
        let mut ip = start;
        while ip <= mf_limit {
            let Some(first) = self.find(ip, ip, limit, MIN_MATCH - 1, attempts, false) else {
                ip += 1;
                continue;
            };
            ip = self.lazy_from(ip, &first, mf_limit, limit, attempts);
        }
    }

    /// Settle the sequences that begin with `first` and return where parsing resumes.
    fn lazy_from(
        &mut self,
        mut ip: usize,
        first: &Found,
        mf_limit: usize,
        limit: usize,
        attempts: usize,
    ) -> usize {
        let (mut ml, mut source) = (first.len, first.pos);
        let (mut start0, mut source0, mut ml0) = (ip, source, ml);
        'second: loop {
            let wider = if ip + ml <= mf_limit {
                self.find(ip + ml - 2, ip, limit, ml, attempts, false)
            } else {
                None
            };
            let Some(w) = wider else {
                self.emit(ip, ml, source);
                return ip + ml;
            };
            let (mut start2, mut source2, mut ml2) = (w.start, w.pos, w.len);
            if start0 < ip && start2 < ip + ml0 {
                (ip, source, ml) = (start0, source0, ml0);
            }
            if start2 - ip < 3 {
                (ip, source, ml) = (start2, source2, ml2);
                continue 'second;
            }
            loop {
                if start2 - ip < OPTIMAL_ML {
                    let mut new_ml = ml.min(OPTIMAL_ML);
                    if ip + new_ml > start2 + ml2 - MIN_MATCH {
                        new_ml = start2 - ip + ml2 - MIN_MATCH;
                    }
                    if new_ml > start2 - ip {
                        let c = new_ml - (start2 - ip);
                        (start2, source2, ml2) = (start2 + c, source2 + c, ml2 - c);
                    }
                }
                let third = if start2 + ml2 <= mf_limit {
                    self.find(start2 + ml2 - 3, start2, limit, ml2, attempts, false)
                } else {
                    None
                };
                let Some(t) = third else {
                    if start2 < ip + ml {
                        ml = start2 - ip;
                    }
                    self.emit(ip, ml, source);
                    self.emit(start2, ml2, source2);
                    return start2 + ml2;
                };
                let (start3, source3, ml3) = (t.start, t.pos, t.len);
                if start3 < ip + ml + 3 {
                    if start3 >= ip + ml {
                        if start2 < ip + ml {
                            let c = ip + ml - start2;
                            (start2, source2) = (start2 + c, source2 + c);
                            ml2 = ml2.saturating_sub(c);
                            if ml2 < MIN_MATCH {
                                (start2, source2, ml2) = (start3, source3, ml3);
                            }
                        }
                        self.emit(ip, ml, source);
                        (ip, source, ml) = (start3, source3, ml3);
                        (start0, source0, ml0) = (start2, source2, ml2);
                        continue 'second;
                    }
                    (start2, source2, ml2) = (start3, source3, ml3);
                    continue;
                }
                if start2 < ip + ml {
                    if start2 - ip < OPTIMAL_ML {
                        ml = ml.min(OPTIMAL_ML);
                        if ip + ml > start2 + ml2 - MIN_MATCH {
                            ml = start2 - ip + ml2 - MIN_MATCH;
                        }
                        if ml > start2 - ip {
                            let c = ml - (start2 - ip);
                            (start2, source2, ml2) = (start2 + c, source2 + c, ml2 - c);
                        }
                    } else {
                        ml = start2 - ip;
                    }
                }
                self.emit(ip, ml, source);
                (ip, source, ml) = (start2, source2, ml2);
                (start2, source2, ml2) = (start3, source3, ml3);
            }
        }
    }

    /// Price-based parsing: for each stretch, record the cheapest way to reach every
    /// position by literals or by a match, then emit the cheapest path through it.
    fn optimal_parse(
        &mut self,
        start: usize,
        attempts: usize,
        sufficient: usize,
        full_update: bool,
    ) {
        let end = self.data.len();
        let mf_limit = end - MF_LIMIT;
        let limit = end - LAST_LITERALS;
        let sufficient = sufficient.min(OPT_NUM - 1);
        let mut opt = vec![Opt::default(); OPT_NUM + TRAILING_LITERALS + 1];
        let mut ip = start;
        while ip <= mf_limit {
            let literals = ip - self.anchor;
            let Some(first) = self.longer(ip, limit, MIN_MATCH - 1, attempts) else {
                ip += 1;
                continue;
            };
            if first.len > sufficient {
                self.emit(ip, first.len, ip - first.off);
                ip += first.len;
                continue;
            }
            for (r, slot) in opt.iter_mut().enumerate().take(MIN_MATCH) {
                *slot = Opt {
                    mlen: 1,
                    off: 0,
                    litlen: literals + r,
                    price: literals_price(literals + r),
                };
            }
            for (mlen, slot) in opt
                .iter_mut()
                .enumerate()
                .take(first.len + 1)
                .skip(MIN_MATCH)
            {
                *slot = Opt {
                    mlen,
                    off: first.off,
                    litlen: literals,
                    price: sequence_price(literals, mlen),
                };
            }
            let mut last = first.len;
            fill_trailing(&mut opt, last);

            let mut cur = 1;
            let mut immediate = None;
            while cur < last {
                if ip + cur > mf_limit {
                    break;
                }
                let skip = if full_update {
                    opt[cur + 1].price <= opt[cur].price
                        && opt[cur + MIN_MATCH].price < opt[cur].price + 3
                } else {
                    opt[cur + 1].price <= opt[cur].price
                };
                if skip {
                    cur += 1;
                    continue;
                }
                let min_len = if full_update {
                    MIN_MATCH - 1
                } else {
                    last - cur
                };
                let Some(m) = self.longer(ip + cur, limit, min_len, attempts) else {
                    cur += 1;
                    continue;
                };
                if m.len > sufficient || m.len + cur >= OPT_NUM {
                    immediate = Some((m.len, m.off));
                    last = cur + 1;
                    break;
                }

                let base = opt[cur].litlen;
                for extra in 1..MIN_MATCH {
                    let price =
                        opt[cur].price - literals_price(base) + literals_price(base + extra);
                    let pos = cur + extra;
                    if price < opt[pos].price {
                        opt[pos] = Opt {
                            mlen: 1,
                            off: 0,
                            litlen: base + extra,
                            price,
                        };
                    }
                }
                for ml in MIN_MATCH..=m.len {
                    let pos = cur + ml;
                    let (ll, price) = if opt[cur].mlen == 1 {
                        let ll = opt[cur].litlen;
                        let before = if cur > ll { opt[cur - ll].price } else { 0 };
                        (ll, before + sequence_price(ll, ml))
                    } else {
                        (0, opt[cur].price + sequence_price(0, ml))
                    };
                    if pos > last + TRAILING_LITERALS || price <= opt[pos].price {
                        if ml == m.len && last < pos {
                            last = pos;
                        }
                        opt[pos] = Opt {
                            mlen: ml,
                            off: m.off,
                            litlen: ll,
                            price,
                        };
                    }
                }
                fill_trailing(&mut opt, last);
                cur += 1;
            }

            let (best_len, best_off, mut candidate) = match immediate {
                Some((len, off)) => (len, off, cur),
                None => (opt[last].mlen, opt[last].off, last - opt[last].mlen),
            };
            let (mut selected_len, mut selected_off) = (best_len, best_off);
            loop {
                let next_len = opt[candidate].mlen;
                let next_off = opt[candidate].off;
                opt[candidate].mlen = selected_len;
                opt[candidate].off = selected_off;
                selected_len = next_len;
                selected_off = next_off;
                if next_len > candidate {
                    break;
                }
                candidate -= next_len;
            }

            let mut rel = 0;
            while rel < last {
                let ml = opt[rel].mlen;
                if ml == 1 {
                    ip += 1;
                    rel += 1;
                    continue;
                }
                let off = opt[rel].off;
                rel += ml;
                self.emit(ip, ml, ip - off);
                ip += ml;
            }
        }
    }
}

struct Longer {
    len: usize,
    off: usize,
}

#[derive(Clone, Copy, Default)]
struct Opt {
    price: isize,
    off: usize,
    mlen: usize,
    litlen: usize,
}

fn fill_trailing(opt: &mut [Opt], last: usize) {
    for add in 1..=TRAILING_LITERALS {
        opt[last + add] = Opt {
            mlen: 1,
            off: 0,
            litlen: add,
            price: opt[last].price + literals_price(add),
        };
    }
}

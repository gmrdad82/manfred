use crate::pyrandom::Random;

#[derive(Clone)]
pub struct RandomState {
    mt: Random,
}

impl RandomState {
    pub fn new(seed: u32) -> RandomState {
        RandomState {
            mt: Random::init_genrand(seed),
        }
    }

    pub fn random_sample(&mut self) -> f64 {
        self.mt.random()
    }

    pub fn rand(&mut self, count: usize) -> Vec<f64> {
        (0..count).map(|_| self.mt.random()).collect()
    }

    pub fn uniform(&mut self, low: f64, high: f64) -> f64 {
        self.mt.uniform(low, high)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: [(u32, [u64; 3]); 15] = [
        (
            11,
            [
                4595662920319748328,
                4581270577588047392,
                4602016223051222140,
            ],
        ),
        (
            12,
            [
                4594722322046499176,
                4604840994620201267,
                4598415081534981236,
            ],
        ),
        (
            21,
            [
                4587182816397575472,
                4598879756542300440,
                4604669107087148005,
            ],
        ),
        (
            22,
            [
                4596678602677501320,
                4602348814518981598,
                4601247359674453674,
            ],
        ),
        (
            23,
            [
                4602834624659138507,
                4606704700404624799,
                4605069868119650455,
            ],
        ),
        (
            24,
            [
                4606822286684422158,
                4604475863960264581,
                4607181223478238114,
            ],
        ),
        (
            25,
            [
                4606012601020237188,
                4603419903863268770,
                4598694735715656794,
            ],
        ),
        (
            31,
            [
                4598824707456458738,
                4606805067289989549,
                4605113581614275642,
            ],
        ),
        (
            32,
            [
                4605911406313543320,
                4600385787192559912,
                4603175375060750654,
            ],
        ),
        (
            33,
            [
                4598121541229146264,
                4601777656472186898,
                4601074471306892134,
            ],
        ),
        (
            34,
            [
                4585718144928539920,
                4605201739831937373,
                4591344430230823304,
            ],
        ),
        (
            35,
            [
                4601923204294454824,
                4599226352618013980,
                4597508220048641468,
            ],
        ),
        (
            36,
            [
                4604737028978490889,
                4603594078630578990,
                4606751700543114643,
            ],
        ),
        (
            44,
            [
                4605694809124564022,
                4592215775808605328,
                4604882344736910142,
            ],
        ),
        (
            46,
            [
                4605235353711295853,
                4603893293229319029,
                4598140743283731608,
            ],
        ),
    ];

    fn bits(values: &[f64]) -> Vec<u64> {
        values.iter().map(|v| v.to_bits()).collect()
    }

    #[test]
    fn rand_matches_numpy_for_every_seed_textures_use() {
        for (seed, want) in FIRST {
            assert_eq!(bits(&RandomState::new(seed).rand(3)), want, "seed {seed}");
        }
    }

    #[test]
    fn uniform_matches_numpy_in_plasters_order() {
        let mut rs = RandomState::new(22);
        assert_eq!(
            bits(&rs.rand(2)),
            [4596678602677501320, 4602348814518981598]
        );
        assert_eq!(rs.uniform(0.06, 0.16).to_bits(), 4592018172208059706);
        assert_eq!(
            rs.uniform(0.0, 2.0 * std::f64::consts::PI).to_bits(),
            4617764076157190840
        );
        assert_eq!(rs.uniform(0.6, 1.6).to_bits(), 4605121225316252385);
        assert_eq!(rs.uniform(0.025, 0.06).to_bits(), 4585472941257199438);
    }

    #[test]
    fn a_million_draws_stay_on_numpys_stream() {
        let mut rs = RandomState::new(24);
        let grid = rs.rand(1024 * 1024);
        assert_eq!(grid[1023 * 1024 + 1023].to_bits(), 4605405131055657348);
        assert_eq!(grid[511 * 1024 + 7].to_bits(), 4605242272276978291);
        assert_eq!(rs.random_sample().to_bits(), 4604876775710418854);
    }

    #[test]
    fn uniform_is_numpys_arithmetic_bit_for_bit() {
        let bounds = [
            (0.06, 0.16),
            (0.0, std::f64::consts::TAU),
            (-3.5, 1e-3),
            (2.0, -7.25),
        ];
        let mut ours = RandomState::new(7);
        let mut numpy = RandomState::new(7);
        for draw in 0..4096 {
            let (low, high) = bounds[draw % bounds.len()];
            let range = high - low;
            let want = low + range * numpy.random_sample();
            assert_eq!(ours.uniform(low, high).to_bits(), want.to_bits(), "{draw}");
        }
    }

    #[test]
    fn the_edge_seeds_match_numpy() {
        assert_eq!(
            bits(&RandomState::new(0).rand(2)),
            [4603118492128842408, 4604617072673064755]
        );
        assert_eq!(
            bits(&RandomState::new(u32::MAX).rand(2)),
            [4591699549773514784, 4606393233629524657]
        );
    }
}

//! A critically damped spring
use tokio::time::Instant;

#[derive(Debug)]
pub(crate) struct Spring {
    last_update: Instant,
    target: f64,
    value: f64,
    velocity: f64,
    halflife: f64,
}

impl Spring {
    pub(crate) fn new(value: f64, target: f64, halflife: f64) -> Self {
        Self {
            value,
            target,
            halflife,
            last_update: Instant::now(),
            velocity: 0.0f64,
        }
    }

    pub(crate) async fn update(&mut self) {
        let now = Instant::now();
        let dt = now - self.last_update;
        self.last_update = now;
        spring_update(
            &mut self.value,
            self.target,
            &mut self.velocity,
            self.halflife,
            dt.as_secs_f64(),
        );
    }

    // pub(crate) fn value(&self) -> f64 {
    //     self.value
    // }

    // pub(crate) fn value_u32(&self) -> u32 {
    //     self.value.max(std::u32::MIN as f64).floor().min(std::u32::MAX as f64) as u32
    // }
    pub(crate) fn value_i64(&self) -> i64 {
        self.value
            .max(std::i64::MIN as f64)
            .floor()
            .min(std::i64::MAX as f64) as i64
    }
    pub(crate) fn value_u64(&self) -> u64 {
        self.value
            .max(std::u64::MIN as f64)
            .floor()
            .min(std::u64::MAX as f64) as u64
    }

    #[allow(dead_code)]
    pub(crate) fn set_target(&mut self, target: f64) {
        self.target = target;
    }

    #[allow(dead_code)] // Used in debug print
    pub(crate) fn get_target(&self) -> f64 {
        self.target
    }

    #[allow(dead_code)]
    pub(crate) fn mod_value(&mut self, delta: f64) {
        self.target += delta;
        self.value += delta;
    }

    pub(crate) fn reset_to(&mut self, target: f64) {
        self.target = target;
        self.value = target;
        self.velocity = 0.0;
    }
}

const EPS: f64 = 1e-5;
fn halflife_to_damping(halflife: f64) -> f64 {
    (4.0 * std::f64::consts::LN_2) / (halflife + EPS)
}

// fn damping_to_halflife(damping: f32) -> f32 {
//     (4.0f32 * std::f32::consts::LN_2) / (damping + EPS)
// }
fn fast_negexp(x: f64) -> f64 {
    1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x)
}

fn spring_update(value: &mut f64, target: f64, velocity: &mut f64, halflife: f64, dt: f64) {
    if halflife.abs() < 1e-3 {
        *value = target;
        *velocity = 0.0;
    } else {
        let y = halflife_to_damping(halflife) / 2.0;
        let j0 = *value - target;
        let j1 = *velocity + j0 * y;
        let eydt = fast_negexp(y * dt);

        *value = eydt * (j0 + j1 * dt) + target;
        *velocity = eydt * (*velocity - j1 * y * dt);
    }
}

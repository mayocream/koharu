use candle_core::{Result, Tensor};

#[derive(Debug, Clone)]
pub struct FlowMatchScheduler {
    sigmas: Vec<f64>,
    timesteps: Vec<f64>,
    step_index: usize,
}

impl FlowMatchScheduler {
    pub fn new(num_steps: usize, image_seq_len: usize) -> Self {
        let num_steps = num_steps.max(1);
        let mu = compute_empirical_mu(image_seq_len, num_steps);
        let mut sigmas = (0..num_steps)
            .map(|i| {
                if num_steps == 1 {
                    1.0
                } else {
                    1.0 - i as f64 * (1.0 - 1.0 / num_steps as f64) / (num_steps - 1) as f64
                }
            })
            .map(|sigma| time_shift(mu, sigma))
            .collect::<Vec<_>>();
        let timesteps = sigmas.iter().map(|sigma| sigma * 1000.0).collect();
        sigmas.push(0.0);
        Self {
            sigmas,
            timesteps,
            step_index: 0,
        }
    }

    pub fn timesteps(&self) -> &[f64] {
        &self.timesteps
    }

    pub fn set_step_index(&mut self, step_index: usize) {
        self.step_index = step_index;
    }

    pub fn timestep_for_model(&self, idx: usize) -> f64 {
        self.timesteps[idx] / 1000.0
    }

    pub fn scale_noise(&self, sample: &Tensor, timestep: f64, noise: &Tensor) -> Result<Tensor> {
        let sigma = timestep / 1000.0;
        (sample * (1.0 - sigma))? + (noise * sigma)?
    }

    pub fn step(&mut self, model_output: &Tensor, sample: &Tensor) -> Result<Tensor> {
        let sigma = self.sigmas[self.step_index];
        let sigma_next = self.sigmas[self.step_index + 1];
        let dt = sigma_next - sigma;
        self.step_index += 1;
        sample + (model_output * dt)?
    }
}

pub fn compute_empirical_mu(image_seq_len: usize, num_steps: usize) -> f64 {
    let image_seq_len = image_seq_len as f64;
    let num_steps = num_steps as f64;

    let a1 = 8.738_095_24e-5;
    let b1 = 1.898_333_33;
    let a2 = 0.000_169_27;
    let b2 = 0.456_666_66;

    if image_seq_len > 4300.0 {
        a2 * image_seq_len + b2
    } else {
        let m_200 = a2 * image_seq_len + b2;
        let m_10 = a1 * image_seq_len + b1;
        let a = (m_200 - m_10) / 190.0;
        let b = m_200 - 200.0 * a;
        a * num_steps + b
    }
}

fn time_shift(mu: f64, sigma: f64) -> f64 {
    if sigma <= 0.0 {
        0.0
    } else {
        let exp_mu = mu.exp();
        exp_mu / (exp_mu + (1.0 / sigma - 1.0))
    }
}

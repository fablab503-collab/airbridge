//! Training, done in Rust:
//!   * logistic regression for the "Train: <Group>/<Label>" albums (like scikit-learn, C = 5)
//!   * the age-ignoring face layer: PCA (200) + LDA (eigen solver, shrinkage 0.3), trained on
//!     FG-NET (the same people photographed from age 0 to 69) plus the faces you confirmed

use anyhow::{bail, Result};
use nalgebra::{DMatrix, SymmetricEigen};

use crate::{faces::Linear, labels::Head};

// ------------------------------------------------------- logistic regression ---
/// Softmax regression with an L2 penalty on the weights (not the offsets), solved with L-BFGS.
/// Two labels use one weight row (sigmoid), like scikit-learn.
pub fn logistic_regression(x: &[Vec<f32>], y: &[String], c: f64) -> Result<Head> {
    let mut classes: Vec<String> = y.to_vec();
    classes.sort();
    classes.dedup();
    if classes.len() < 2 {
        bail!("needs at least 2 labels");
    }
    let (n, d) = (x.len(), x[0].len());
    let rows = if classes.len() == 2 { 1 } else { classes.len() };
    let target: Vec<usize> = y.iter().map(|l| classes.iter().position(|c| c == l).unwrap()).collect();
    let xs: Vec<Vec<f64>> = x.iter().map(|r| r.iter().map(|&v| v as f64).collect()).collect();
    let per_row = d + 1; // weights then offset

    // average loss + penalty / (2 C n), and its gradient
    let f = |p: &[f64], g: &mut [f64]| -> f64 {
        g.iter_mut().for_each(|v| *v = 0.0);
        let mut loss = 0.0;
        let mut z = vec![0.0; rows + 1];
        for (xi, &t) in xs.iter().zip(&target) {
            // logits; with 2 labels the first label's logit is fixed at 0
            let logits = &mut z;
            if rows == 1 {
                logits[0] = 0.0;
                logits[1] = p[d] + xi.iter().zip(&p[..d]).map(|(a, b)| a * b).sum::<f64>();
            } else {
                for k in 0..rows {
                    let w = &p[k * per_row..k * per_row + d];
                    logits[k] = p[k * per_row + d] + xi.iter().zip(w).map(|(a, b)| a * b).sum::<f64>();
                }
            }
            let k_all = if rows == 1 { 2 } else { rows };
            let m = logits[..k_all].iter().cloned().fold(f64::MIN, f64::max);
            let s: f64 = logits[..k_all].iter().map(|v| (v - m).exp()).sum();
            loss += m + s.ln() - logits[t];
            for k in 0..k_all {
                let prob = (logits[k] - m).exp() / s;
                let err = prob - if k == t { 1.0 } else { 0.0 };
                let r = if rows == 1 { if k == 0 { continue } else { 0 } } else { k };
                let gw = &mut g[r * per_row..r * per_row + per_row];
                for (gj, xj) in gw[..d].iter_mut().zip(xi) {
                    *gj += err * xj;
                }
                gw[d] += err;
            }
        }
        let nf = n as f64;
        let mut penalty = 0.0;
        for r in 0..rows {
            for j in 0..d {
                let w = p[r * per_row + j];
                penalty += w * w;
                g[r * per_row + j] = g[r * per_row + j] / nf + w / (c * nf);
            }
            g[r * per_row + d] /= nf;
        }
        loss / nf + penalty / (2.0 * c * nf)
    };
    let p = lbfgs(vec![0.0; rows * per_row], f, 3000)?;
    let coef = (0..rows).map(|r| p[r * per_row..r * per_row + d].iter().map(|&v| v as f32).collect()).collect();
    let intercept = (0..rows).map(|r| p[r * per_row + d] as f32).collect();
    Ok(Head { classes, coef, intercept })
}

/// Limited-memory BFGS: finds the lowest point of a smooth bowl-shaped function.
fn lbfgs(mut x: Vec<f64>, f: impl Fn(&[f64], &mut [f64]) -> f64, max_iter: usize) -> Result<Vec<f64>> {
    const M: usize = 10;
    let dotp = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(p, q)| p * q).sum::<f64>();
    let mut g = vec![0.0; x.len()];
    let mut fx = f(&x, &mut g);
    let mut hist: Vec<(Vec<f64>, Vec<f64>, f64)> = vec![]; // (s, y, 1/(y.s))
    for _ in 0..max_iter {
        if g.iter().fold(0.0f64, |m, v| m.max(v.abs())) < 1e-7 {
            break;
        }
        // direction from the remembered curvature (two-loop recursion)
        let mut q = g.clone();
        let mut alphas = vec![];
        for (s, y, rho) in hist.iter().rev() {
            let a = rho * dotp(s, &q);
            q.iter_mut().zip(y).for_each(|(qi, yi)| *qi -= a * yi);
            alphas.push(a);
        }
        if let Some((s, y, _)) = hist.last() {
            let gamma = dotp(s, y) / dotp(y, y);
            q.iter_mut().for_each(|v| *v *= gamma);
        }
        for ((s, y, rho), a) in hist.iter().zip(alphas.iter().rev()) {
            let b = rho * dotp(y, &q);
            q.iter_mut().zip(s).for_each(|(qi, si)| *qi += (a - b) * si);
        }
        let dir: Vec<f64> = q.iter().map(|v| -v).collect();
        let slope = dotp(&g, &dir);
        if slope >= 0.0 {
            hist.clear();
            continue;
        }
        // step back until the value goes down enough
        let mut step = if hist.is_empty() { 1.0 / g.iter().map(|v| v * v).sum::<f64>().sqrt().max(1.0) } else { 1.0 };
        let mut g_new = vec![0.0; x.len()];
        let mut accepted = None;
        for _ in 0..40 {
            let x_new: Vec<f64> = x.iter().zip(&dir).map(|(a, b)| a + step * b).collect();
            let f_new = f(&x_new, &mut g_new);
            if f_new <= fx + 1e-4 * step * slope {
                accepted = Some((x_new, f_new));
                break;
            }
            step *= 0.5;
        }
        let Some((x_new, f_new)) = accepted else { break };
        let s: Vec<f64> = x_new.iter().zip(&x).map(|(a, b)| a - b).collect();
        let y: Vec<f64> = g_new.iter().zip(&g).map(|(a, b)| a - b).collect();
        let ys = dotp(&y, &s);
        if ys > 1e-12 {
            hist.push((s, y, 1.0 / ys));
            if hist.len() > M {
                hist.remove(0);
            }
        }
        let done = (fx - f_new).abs() < 1e-12 * fx.abs().max(1.0);
        x = x_new;
        fx = f_new;
        g = g_new;
        if done {
            break;
        }
    }
    Ok(x)
}

// --------------------------------------------------------- age-ignoring layer ---
/// scikit-learn's covariance with a fixed shrinkage: (1 - s) * covariance + s * (average variance) * I.
fn shrunk_cov(x: &DMatrix<f64>, shrinkage: f64) -> DMatrix<f64> {
    let (n, p) = (x.nrows(), x.ncols());
    let mean = x.row_mean();
    let centered = DMatrix::from_fn(n, p, |i, j| x[(i, j)] - mean[j]);
    let emp = centered.transpose() * &centered / n as f64;
    let mu = emp.trace() / p as f64;
    let mut cov = emp * (1.0 - shrinkage);
    for j in 0..p {
        cov[(j, j)] += shrinkage * mu;
    }
    cov
}

/// Train the layer: face numbers of the same person (at any age) end up close together.
pub fn age_invariant(x: &[Vec<f32>], people: &[String], pca_dims: usize, shrinkage: f64) -> Result<Linear> {
    let (n, d) = (x.len(), x[0].len());
    let xm = DMatrix::from_fn(n, d, |i, j| x[i][j] as f64);
    // PCA: keep the 200 directions where faces differ most
    let mean = xm.row_mean();
    let centered = DMatrix::from_fn(n, d, |i, j| xm[(i, j)] - mean[j]);
    let eig = SymmetricEigen::new(centered.transpose() * &centered);
    let mut order: Vec<usize> = (0..d).collect();
    order.sort_by(|&a, &b| eig.eigenvalues[b].total_cmp(&eig.eigenvalues[a]));
    let k = pca_dims.min(d).min(n);
    let pca = DMatrix::from_fn(d, k, |i, j| eig.eigenvectors[(i, order[j])]);
    let z = &centered * &pca;

    // LDA: directions that separate different people, ignoring the differences within one person
    let mut classes: Vec<&String> = people.iter().collect();
    classes.sort();
    classes.dedup();
    if classes.len() < 2 {
        bail!("needs faces of at least 2 people");
    }
    let mut sw = DMatrix::<f64>::zeros(k, k);
    for c in &classes {
        let idx: Vec<usize> = (0..n).filter(|&i| &people[i] == *c).collect();
        let zc = DMatrix::from_fn(idx.len(), k, |i, j| z[(idx[i], j)]);
        sw += shrunk_cov(&zc, shrinkage) * (idx.len() as f64 / n as f64);
    }
    let sb = shrunk_cov(&z, shrinkage) - &sw;
    // generalised eigenproblem Sb v = l Sw v, through the Cholesky factor of Sw
    let chol = sw.clone().cholesky().ok_or_else(|| anyhow::anyhow!("face data is degenerate"))?;
    let l_inv = chol.l().try_inverse().ok_or_else(|| anyhow::anyhow!("face data is degenerate"))?;
    let c = &l_inv * &sb * l_inv.transpose();
    let c = (&c + c.transpose()) * 0.5;
    let eig = SymmetricEigen::new(c);
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by(|&a, &b| eig.eigenvalues[b].total_cmp(&eig.eigenvalues[a]));
    let out = (classes.len() - 1).min(k);
    let vecs = l_inv.transpose() * DMatrix::from_fn(k, out, |i, j| eig.eigenvectors[(i, order[j])]);

    // the whole thing is one straight-line map: x -> (x - mean) * PCA * LDA
    let a = &pca * &vecs;
    let offset = -(mean * &a);
    Ok(Linear {
        input: d,
        output: out,
        matrix: (0..d).flat_map(|i| (0..out).map(move |j| (i, j))).map(|(i, j)| a[(i, j)] as f32).collect(),
        offset: offset.iter().map(|&v| v as f32).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logistic_learns_simple_split() {
        // three clouds of points; the model must put each new point in its cloud
        let mut x = vec![];
        let mut y = vec![];
        let mut seed = 1u64;
        let mut rnd = || { seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); ((seed >> 33) as f32 / (1u64 << 31) as f32) - 0.5 };
        for (label, cx, cy) in [("a", 1.0, 0.0), ("b", -1.0, 0.0), ("c", 0.0, 1.0)] {
            for _ in 0..30 {
                x.push(vec![cx + rnd() * 0.5, cy + rnd() * 0.5, rnd()]);
                y.push(label.to_string());
            }
        }
        let head = logistic_regression(&x, &y, 5.0).unwrap();
        let p = head.predict_proba(&[0.95, 0.05, 0.0]);
        assert!(p[0] > 0.8, "{p:?}");
        let two: Vec<String> = y.iter().map(|l| if l == "a" { "a".into() } else { "other".into() }).collect();
        let head = logistic_regression(&x, &two, 5.0).unwrap();
        assert_eq!(head.coef.len(), 1);
        assert!(head.predict_proba(&[1.0, 0.0, 0.0])[0] > 0.8);
    }
}

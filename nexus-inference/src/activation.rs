/// Activation function for hidden layers and convolution outputs.
///
/// Applied element-wise. All variants use pure arithmetic
/// approximations — no libm or runtime math library required.
///
/// The compute kernels live in [`crate::kernel::activate`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Activation {
    /// max(0, x)
    Relu,
    /// x if x >= 0, alpha * x otherwise.
    LeakyRelu(f32),
    /// Hyperbolic tangent.
    Tanh,
    /// 1 / (1 + exp(-x)).
    Sigmoid,
    /// Pass-through (no transformation).
    Identity,
    /// x if x >= 0, alpha * (exp(x) - 1) otherwise.
    Elu(f32),
    /// Gaussian error linear unit (tanh approximation).
    Gelu,
    /// x * sigmoid(x), also known as SiLU in PyTorch.
    Swish,
}

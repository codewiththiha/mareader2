//! Animation properties shared by the source schema and generated geometry.

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prop {
    X,
    Y,
    Scale,
    ScaleX,
    ScaleY,
    Rotate,
    SkewX,
    SkewY,
    Opacity,
    PathLen,
    PathOff,
    StrokeW,
}

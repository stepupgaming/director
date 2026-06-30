pub mod visual;

#[cfg(test)]
mod elements {
    include!("visual/elements.rs");
}

#[cfg(test)]
mod layout {
    include!("visual/layout.rs");
}

#[cfg(test)]
mod text {
    include!("visual/text.rs");
}

#[cfg(test)]
mod images {
    include!("visual/images.rs");
}

#[cfg(test)]
mod animations {
    include!("visual/animations.rs");
}

#[cfg(test)]
mod effects {
    include!("visual/effects.rs");
}

#[cfg(test)]
mod border_padding {
    include!("visual/border_padding.rs");
}

#[cfg(test)]
mod templates {
    include!("visual/templates.rs");
}

#[cfg(test)]
mod enforce_template_coverage {
    include!("visual/enforce_template_coverage.rs");
}

#[cfg(test)]
mod temporal {
    include!("visual/temporal.rs");
}

#[cfg(test)]
mod z_index {
    include!("visual/z_index.rs");
}

#[cfg(test)]
mod text_animators {
    include!("visual/text_animators.rs");
}

#[cfg(test)]
mod composition {
    include!("visual/composition.rs");
}

#[cfg(test)]
mod masking {
    include!("visual/masking.rs");
}

#[cfg(test)]
mod shapes {
    include!("visual/shapes.rs");
}

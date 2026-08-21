use crate::signing::{validate_nested_components, SignatureKind, SignedComponent};

pub fn validate_generic_nested_components(
    components: &[SignedComponent],
) -> Result<(), String> {
    for component in components {
        if component.kind != SignatureKind::Other {
            validate_nested_components(std::slice::from_ref(component))?;
            continue;
        }
        if component.identifier.is_none()
            || component.team_identifier.is_none()
            || component.authorities.is_empty()
            || !component.verified
        {
            return Err(format!(
                "third-party nested component lacks identity evidence: {}",
                component.path.display()
            ));
        }
    }
    Ok(())
}

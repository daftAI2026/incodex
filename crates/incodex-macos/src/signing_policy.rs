use crate::signature_inspection::has_identity_evidence;
use crate::signing::{validate_nested_components, SignatureKind, SignedComponent};

pub fn validate_generic_nested_components(components: &[SignedComponent]) -> Result<(), String> {
    for component in components {
        if component.kind != SignatureKind::Other {
            validate_nested_components(std::slice::from_ref(component))?;
            continue;
        }
        if !has_identity_evidence(component) || !component.verified {
            return Err(format!(
                "third-party nested component lacks identity evidence: {}",
                component.path.display()
            ));
        }
    }
    Ok(())
}

pub(crate) mod api;
pub(crate) mod cluster;

#[cfg(test)]
pub(crate) mod tests {
    use crate::domain::mpc::api::Server;

    use std::fs::read_to_string;

    pub fn load_cluster_from_config() -> anyhow::Result<Vec<Vec<Server>>> {
        let str = read_to_string("integration-tests/test-data/cluster-config.yml")?;
        let result = serde_yaml::from_str(&str)?;
        Ok(result)
    }
}

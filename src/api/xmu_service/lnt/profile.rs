use crate::abi::utils::SmartJsonExt;
use ahash::RandomState;
use anyhow::Result;
use dashmap::DashMap;
use serde::Deserialize;
//use serde::de::IgnoredAny;
use helper::lnt_get_api;
use std::sync::{Arc, LazyLock};

#[derive(Deserialize, Debug)]
pub struct Department {
    pub id: i64,
    pub name: String,
    //pub code: IgnoredAny,
    //pub cover: IgnoredAny,
    //pub created_at: IgnoredAny,
    //pub created_user: IgnoredAny,
    //pub is_show_on_homepage: IgnoredAny,
    //pub parent_id: IgnoredAny,
    //pub short_name: IgnoredAny,
    //pub sort: IgnoredAny,
    //pub stopped: IgnoredAny,
    //pub storage_assigned: IgnoredAny,
    //pub storage_used: IgnoredAny,
    //pub updated_at: IgnoredAny,
    //pub updated_user: IgnoredAny,
}

#[derive(Deserialize, Debug)]
pub struct ProfileResponse {
    pub id: i64,
    pub name: String,
    pub user_no: String,
    pub department: Department,
    //pub active: IgnoredAny,
    //pub audit: IgnoredAny,
    //pub avatar_big_url: IgnoredAny,
    //pub avatar_small_url: IgnoredAny,
    //pub comment: IgnoredAny,
    //pub created_at: IgnoredAny,
    //pub created_by: IgnoredAny,
    //pub education: IgnoredAny,
    //pub email: IgnoredAny,
    //pub end_at: IgnoredAny,
    //pub grade: IgnoredAny,
    //pub has_ai_ability: IgnoredAny,
    //pub imported_from: IgnoredAny,
    //pub is_imported_data: IgnoredAny,
    //pub klass: IgnoredAny,
    //pub mobile_phone: IgnoredAny,
    //pub nickname: IgnoredAny,
    //pub org: IgnoredAny,
    //pub program: IgnoredAny,
    //pub program_id: IgnoredAny,
    //pub remarks: IgnoredAny,
    //pub require_verification: IgnoredAny,
    //pub role: IgnoredAny,
    //pub user_addresses: IgnoredAny,
    //pub user_attributes: IgnoredAny,
    //pub user_auth_externals: IgnoredAny,
    //pub user_personas: IgnoredAny,
    //pub webex_auth: IgnoredAny,
}

#[lnt_get_api(ProfileResponse, "https://lnt.xmu.edu.cn/api/profile")]
pub struct ProfileWithoutCache;

static PROFILE: LazyLock<ProfileStruct> = LazyLock::new(ProfileStruct::new);

pub struct ProfileStruct {
    pub profile_data: DashMap<String, Arc<ProfileResponse>, RandomState>,
}

impl Default for ProfileStruct {
    fn default() -> Self {
        Self::new()
    }
}

impl ProfileStruct {
    pub fn new() -> Self {
        ProfileStruct {
            profile_data: DashMap::with_hasher(RandomState::default()),
        }
    }

    pub async fn get_profile(&self, session: &str) -> Result<Arc<ProfileResponse>> {
        if let Some(entry) = self.profile_data.get(session) {
            return Ok((*entry.value()).clone());
        }

        let user_info = ProfileWithoutCache::get(session).await?;
        let user_info = Arc::new(user_info);

        self.profile_data
            .insert(session.to_string(), user_info.clone());
        Ok(user_info)
    }
}

pub struct Profile;

impl Profile {
    pub async fn get(session: &str) -> Result<Arc<ProfileResponse>> {
        PROFILE.get_profile(session).await
    }

    pub async fn check(session: &str) -> bool {
        (Self::get(session).await).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test_error() -> Result<()> {
        let castgc = "TGT-2435869-O8Wwbqik8mV2AiaFWm2RKkKG8nq1zARLvjuN2XWuYtBMaXNrSUaZDng4bJZj-3FfQrsnull_main";
        let session = castgc_get_session(castgc).await?;
        let profile = Profile::get(&session).await?;
        println!("Profile: {:?}", profile);
        let check_result = Profile::check(&session).await;
        println!("Check Result: {}", check_result);
        Ok(())
    }

    #[tokio::test]
    async fn test_success() -> Result<()> {
        let castgc = "TGT-4073508-WHsRVSCV2-j9q5z3D2VXbcR8-ZFkHzsltAKa7aioXRvKY8fRACTJatRxjSdJtdbsRiInull_main";
        let session = castgc_get_session(castgc).await?;
        let profile = Profile::get(&session).await?;
        println!("Profile: {:?}", profile);
        let check_result = Profile::check(&session).await;
        println!("Check Result: {}", check_result);
        Ok(())
    }

    #[tokio::test]
    async fn test_no_cache() -> Result<()> {
        let castgc = "TGT-3154081-z-6MPScC0VhX-By3-gpG2wXuI4ix7KILP96lGe7Jb9GUDDoEz9g-0IEsfTrDtAU9rBInull_main";
        let session = castgc_get_session(castgc).await?;
        let profile = ProfileWithoutCache::get(&session).await?;
        println!("Profile: {:?}", profile);
        Ok(())
    }
}

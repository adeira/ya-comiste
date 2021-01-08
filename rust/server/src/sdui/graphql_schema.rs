use crate::arangodb::errors::ModelError;
use crate::auth::users::User;
use crate::sdui::graphql_context::Context;
use crate::sdui::model::sdui_sections::get_all_sections_for_entrypoint_key;
use crate::sdui::sdui_section::SDUISection;
use juniper::{EmptySubscription, FieldError, FieldResult, RootNode};

#[derive(Clone, Copy, Debug)]
pub struct Query;

#[derive(juniper::GraphQLObject)]
struct WhoamiPayload {
    id: Option<juniper::ID>,

    /// Human readable type should be used only for testing purposes. The format is not guaranteed
    /// and can change in the future completely.
    human_readable_type: Option<String>,
}

#[juniper::graphql_object(context = Context)]
impl Query {
    async fn mobile_entrypoint_sections(
        key: String,
        context: &Context,
    ) -> FieldResult<Vec<SDUISection>> {
        let connection_pool = context.pool.to_owned();
        match get_all_sections_for_entrypoint_key(&context.user, &connection_pool, &key).await {
            Ok(s) => Ok(s),
            // Err(e) => Err(FieldError::from(e)),
            Err(e) => match e {
                ModelError::DatabaseError(e) => Err(FieldError::from(e)), // TODO: hide and log these errors
                ModelError::LogicError(e) => Err(FieldError::from(e)),
                ModelError::SerdeError(e) => Err(FieldError::from(e)),
            },
        }
    }

    /// Returns information about the current user (can be authenticated or anonymous).
    async fn whoami(context: &Context) -> WhoamiPayload {
        match &context.user {
            User::AuthorizedUser(user) => WhoamiPayload {
                id: Some(juniper::ID::from(user.id())),
                human_readable_type: Some(String::from("authorized user")),
            },
            User::AnonymousUser(user) => WhoamiPayload {
                id: Some(juniper::ID::from(user.id())),
                human_readable_type: Some(String::from("anonymous user")),
            },
            User::UnauthorizedUser(user) => WhoamiPayload {
                id: Some(juniper::ID::from(user.id())),
                human_readable_type: Some(String::from("unauthorized (but not anonymous) user")),
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Mutation;

#[derive(juniper::GraphQLObject)]
struct AuthorizeMobilePayload {
    success: bool,

    /// Session token should be send with every GraphQL request which requires auth.
    /// Returns `None` if the request was not successful.
    session_token: Option<String>,
}

#[derive(juniper::GraphQLObject)]
struct DeauthorizeMobilePayload {
    success: bool,
}

#[juniper::graphql_object(context = Context)]
impl Mutation {
    /// This function accepts Google ID token (after receiving it from Google Sign-In in a mobile
    /// device) and returns authorization payload. There is no concept of sign-in and sign-up
    /// because every user with a valid JWT ID token will be either authorized OR registered and
    /// authorized. Invalid tokens and disabled tokens will be rejected.
    async fn authorize_mobile(
        google_id_token: String,
        context: &Context,
    ) -> FieldResult<AuthorizeMobilePayload> {
        let connection_pool = context.pool.to_owned();
        let session_token = crate::auth::authorize(&connection_pool, &google_id_token).await;
        match session_token {
            Ok(session_token) => Ok(AuthorizeMobilePayload {
                success: true,
                session_token: Some(session_token),
            }),
            Err(e) => {
                log::error!("{}", e);
                Ok(AuthorizeMobilePayload {
                    success: false,
                    session_token: None,
                    // TODO: return rejection reason from AuthError as well (?)
                })
            }
        }
    }

    /// The purpose of this `deauthorize` mutation is to remove the active sessions and efectivelly
    /// make the mobile application unsigned. Mobile applications should remove the session token
    /// once deauthorized.
    async fn deauthorize_mobile(
        session_token: String,
        context: &Context,
    ) -> DeauthorizeMobilePayload {
        let connection_pool = context.pool.to_owned();
        match crate::auth::deauthorize(&connection_pool, &session_token).await {
            Ok(_) => DeauthorizeMobilePayload { success: true },
            Err(_) => DeauthorizeMobilePayload { success: false },
        }
    }
}

pub type Schema = RootNode<'static, Query, Mutation, EmptySubscription<Context>>;

pub fn create_graphql_schema() -> Schema {
    Schema::new(Query, Mutation, EmptySubscription::<Context>::new())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    #[test]
    fn schema_snapshot() {
        // This test will make sure that the schema generated by server is ready to be used by
        // clients. How to do it better?

        let new_schema_path = "./../../ya-comiste-meta/schema.graphql.new";
        let saved_schema_path = "./../../ya-comiste-meta/schema.graphql";
        let saved_schema_snapshot =
            fs::read_to_string(saved_schema_path).expect("unable to read schema file");

        assert!(signedsource::is_signed(&saved_schema_snapshot));
        assert!(signedsource::is_valid_signature(&saved_schema_snapshot));

        let new_schema_snapshot = signedsource::sign_file(&format!(
            "# {}\n\n{}",
            signedsource::SIGNING_TOKEN,
            super::create_graphql_schema().as_schema_language()
        ));

        if saved_schema_snapshot != new_schema_snapshot {
            fs::write(new_schema_path, new_schema_snapshot)
                .expect("unable to write new schema file");
        }

        assert_eq!(
            Path::new(new_schema_path).exists(),
            false,
            "schema snapshot with *.new extension should not exist - please resolve it"
        );
    }
}
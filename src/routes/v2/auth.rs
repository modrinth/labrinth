/*!
This auth module is how we allow for authentication within the Modrinth sphere.
It uses a self-hosted Ory Kratos instance on the backend, powered by our Minos backend.

 Applications interacting with the authenticated API (a very small portion - notifications, private projects, editing/creating projects
and versions) should include the Ory authentication cookie in their requests. This cookie is set by the Ory Kratos instance and Minos provides function to access these.

In addition, you can use a logged-in-account to generate a PAT.
This token can be passed in as a Bearer token in the Authorization header, as an alternative to a cookie.
This is useful for applications that don't have a frontend, or for applications that need to access the authenticated API on behalf of a user.

Just as a summary: Don't implement this flow in your application!
*/

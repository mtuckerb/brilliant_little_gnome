# Brilliant Little Gnome
a Brightspace Student Helper

This is a Proof of Concept (PoC) Ruby script to interact with the D2L Brightspace API.
It implements the OAuth 2.0 flow to authenticate a student and retrieve their course enrollments.

## Prerequisites

1.  **Ruby**: Installed on your system.
2.  **Brightspace Account**: Access to a generic Brightspace instance.
3.  **App Credentials**: You must register an OAuth 2.0 application in Brightspace.

## Setup: Getting Credentials

To use this tool, you need a **Client ID** and **Client Secret**.

### Option A: You are an Administrator
1.  Log in to your Brightspace instance.
2.  Go to **Admin Tools** (gear icon) -> **Manage Extensibility**.
3.  Click **OAuth 2.0** tab.
4.  Click **Register an App**.
5.  Fill in the details:
    *   **Application Name**: Student Dashboard PoC
    *   **Redirect URI**: `https://localhost/callback`
    *   **Scope**: `core:*:* enrollments:*:* content:*:* grades:*:*`
    *   **AccessToken Lifetime**: Default (3600)
6.  Save.
7.  Copy the **Client ID** and **Client Secret**.

### Option B: You are a Student or Instructor
You generally **cannot** generate these keys yourself.
1.  Contact your university's LMS/Brightspace Administrator.
2.  Request an "OAuth 2.0 App Registration" for a student tool.
3.  Provide them with the **Redirect URI** (`https://localhost/callback`) and **Scopes** listed above.

## Usage

1.  Open your terminal.
2.  Export your configuration as environment variables:

    ```bash
    export BS_HOST="your-school.brightspace.com"       # without https://
    export BS_CLIENT_ID="your_client_id_here"
    export BS_CLIENT_SECRET="your_client_secret_here"
    # export BS_REDIRECT_URI="https://localhost/callback" # Only if you changed it registration
    ```

3.  Run the script:

    ```bash
    ruby brightspace_poc.rb
    ```

4.  **Follow the on-screen instructions**:
    *   The script will print a URL.
    *   Open that URL in your browser.
    *   Log in to Brightspace.
    *   Click "Accept" if prompted.
    *   You will be redirected to a page that fails to load (because we aren't running a server on localhost). **This is expected.**
    *   Look at the URL bar. It will look like: `https://localhost/callback?code=jaiu12312kkjasd...`
    *   Copy the value of the `code` parameter (part after `code=`).
    *   Paste it into the terminal prompt.

5.  The script will verify the token and list your courses.

## Features Implemented

*   **OAuth 2.0 Authentication**: Securely gets an Access Token.
*   **WhoAmI**: Verifies identity.
*   **Get Enrollments**: Lists all visible course offerings for the user.

## Next Steps to Build the Full Tool

To expand this into a full "Student Tool":
1.  **Add Endpoints**: 
    *   Add `get_course_content(org_unit_id)` using `/d2l/api/le/1.40/{id}/content/toc`.
    *   Add `get_assignments(org_unit_id)` using `/d2l/api/le/1.40/{id}/dropbox/folders/`.
    *   Add `get_grades(org_unit_id)` using `/d2l/api/le/1.40/{id}/grades/final/values/myGradeValues/`.
2.  **UI**: Wrap this logic in a local web server (using Sinatra or Rails) or a TUI (Text User Interface).

# Your own Google OAuth client

Drivebridge signs in to Google with an OAuth client that you create in your own Google Cloud
project. Nothing passes through anyone else's app, and there is no shared key to be rate limited
or revoked. It takes about ten minutes, once.

1. Open the [Google Cloud Console](https://console.cloud.google.com/) and create a project, for
   example "Drivebridge".
2. In **APIs & Services → Library**, enable the **Google Drive API**.
3. In **APIs & Services → OAuth consent screen**, choose **External**, fill in the app name and
   your email, and leave it in **Testing**. Under **Test users**, add the Google accounts you
   will sync.
4. In **APIs & Services → Credentials**, choose **Create credentials → OAuth client ID**, type
   **Desktop app**. Download the JSON file.
5. Give that file to the Drivebridge setup wizard, or to
   `drivebridge setup --client-secret <file>`.

Keep the file private and never commit it to a repository.

## Good to know

- In Testing mode an app can have up to 100 test users, which covers personal use and a beta.
- Google expires sign-ins of apps in Testing mode after 7 days. When that happens, run setup
  again; your files and sync state are kept.

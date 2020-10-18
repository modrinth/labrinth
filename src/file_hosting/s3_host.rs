use crate::file_hosting::{DeleteFileData, FileHost, FileHostingError, UploadFileData};
use async_trait::async_trait;
use s3::bucket::Bucket;

pub struct S3Host {
    pub bucket: Bucket,
}

#[async_trait]
impl FileHost for S3Host {
    async fn upload_file(
        &self,
        content_type: &str,
        file_name: &str,
        file_bytes: Vec<u8>,
    ) -> Result<UploadFileData, FileHostingError> {
        let content_sha1 = sha1::Sha1::from(&file_bytes).hexdigest();

        self.bucket
            .put_object_with_content_type(
                format!("/{}", file_name),
                file_bytes.as_slice(),
                content_type,
            )
            .await?;

        Ok(UploadFileData {
            file_id: file_name.to_string(),
            file_name: file_name.to_string(),
            content_length: file_bytes.len() as u32,
            content_sha1,
            content_md5: Some("".to_string()),
            content_type: content_type.to_string(),
            upload_timestamp: chrono::Utc::now().timestamp_millis() as u64,
        })
    }

    async fn delete_file_version(
        &self,
        file_id: &str,
        file_name: &str,
    ) -> Result<DeleteFileData, FileHostingError> {
        self.bucket.delete_object(file_name).await?;

        Ok(DeleteFileData {
            file_id: file_id.to_string(),
            file_name: file_name.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::file_hosting::s3_host::S3Host;
    use crate::file_hosting::FileHost;
    use s3::bucket::Bucket;
    use s3::creds::Credentials;
    use s3::region::Region;

    #[actix_rt::test]
    async fn test_file_management() {
        let mut bucket = Bucket::new(
            &*dotenv::var("S3_BUCKET_NAME").unwrap(),
            Region::Custom {
                region: dotenv::var("S3_REGION").unwrap(),
                endpoint: dotenv::var("S3_URL").unwrap(),
            },
            Credentials::new(
                Some(&*dotenv::var("S3_ACCESS_TOKEN").unwrap()),
                Some(&*(dotenv::var("S3_SECRET")).unwrap()),
                None,
                None,
                None,
            )
            .unwrap(),
        )
        .unwrap();

        bucket.add_header("x-amz-acl", "public-read");

        let s3_host = S3Host { bucket };

        s3_host
            .upload_file(
                "text/plain",
                "test.txt",
                "test file".to_string().into_bytes(),
            )
            .await
            .unwrap();

        s3_host.delete_file_version("", "test.txt").await.unwrap();
    }
}

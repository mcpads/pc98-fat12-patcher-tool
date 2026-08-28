use crate::recipe::UnsupportedPackageFormat;

const UNSUPPORTED_PACKAGE_MESSAGE: &str =
    "이 패치 ZIP은 현재 지원하는 파일별 패치 형식이 아닙니다. 이 패처용 ZIP을 선택하세요.";
const INVALID_PACKAGE_MESSAGE: &str =
    "패치 ZIP을 읽을 수 없습니다. 올바른 PC-98 FAT12 패치 ZIP인지 확인하세요.";

pub(crate) fn describe_package_selection_failure(error: anyhow::Error) -> String {
    if error
        .chain()
        .any(|cause| cause.downcast_ref::<UnsupportedPackageFormat>().is_some())
    {
        UNSUPPORTED_PACKAGE_MESSAGE.to_owned()
    } else {
        INVALID_PACKAGE_MESSAGE.to_owned()
    }
}

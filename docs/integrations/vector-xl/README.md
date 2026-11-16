# Vector XL Driver Library (VN5640)

Loom의 VN5640 Ethernet 백엔드 개발에 사용할 Windows SDK 확보 안내.
현재 백엔드 구현이나 실장비 검증을 완료했다는 의미는 아니다.

## 공식 다운로드

- 제품/SDK: https://www.vector.com/en/business-unit/testing-validation-diagnostics/products/network-interfaces/xl-driver-library/
- 다운로드: https://www.vector.com/en/services/download-center/#search:xl%20driver%20library
- 확인한 버전: **26.30.16**, 게시일 **2026-07-23**
- 공식 표시 용량: **288.5 MB**, ZIP
- 공식 표시 SHA-1: `03c53375fdf8527bf05b019ed000ea99666ed067`
- 확인일: **2026-11-16**

다운로드 센터에서 해당 버전을 선택하고 `Request the download link`를 누른다.
확인 시점에는 이름, 이메일, 국가/지역, 약관 동의 및 CAPTCHA가 있는 신청 화면이었다.
사용자가 직접 신청하여 받은 원본은 저장소 밖의 로컬 SDK 디렉터리에 보관한다.
현재 ZIP은 다운로드하지 못했으며, 위 SHA-1은 공식 화면에서 읽은 값이다.
다운로드한 파일에 대해 계산한 해시로 오인하지 말 것.

Windows PowerShell 검증:

```powershell
$sdkZip = 'C:\SDKs\Vector\downloaded-package.zip' # 실제 받은 파일 경로
$expected = '03c53375fdf8527bf05b019ed000ea99666ed067'
$actual = (Get-FileHash -LiteralPath $sdkZip -Algorithm SHA1).Hash.ToLowerInvariant()
if ($actual -ne $expected) { throw "XL Driver Library archive checksum mismatch" }
Get-FileHash -LiteralPath $sdkZip -Algorithm SHA256
```

SHA-1은 공급자 표시값과의 일치 확인용이다. 공급자 출처를 확인하고,
다운로드 후 SHA-256도 기록한다.

## 개발에 필요한 구성

공식 제품 페이지에 명시된 제공 구성:

- `vxlapi64.dll`: 64비트 Windows 런타임
- `vxlapi.dll`: 32비트 Windows 런타임
- `vxlapi.h`: C/C++ API 헤더
- .NET wrapper
- PDF 매뉴얼 및 예제 소스

Vector Driver Setup(장치 드라이버)과 XL Driver Library(SDK)를 구분한다.
패키지를 확보하면 실제 디렉터리 구조와 Ethernet 예제를 확인한다.
Loom은 설치된 DLL을 명시적 경로에서 로드하는 방향으로 설계하며,
SDK DLL을 애플리케이션 배포물에 자동 포함하지 않는다.
VN5640의 Ethernet 접근 모드 및 드라이버/펌웨어 호환성은 해당 버전의
매뉴얼과 Vector Hardware Config에서 확인해야 한다.

## 저장소에 SDK를 포함하지 않는 이유

공식 약관: https://cdn.vector.com/cms/content/corporate/terms-and-conditions/TC_XLDriverLibrary_EN.pdf

확인한 문서는 **Version 1.2, 2025-04-01**이다.
제1조는 DLL의 단독 배포와 프로그램에 포함한 배포 모두에 대해
Vector와의 사전 개별 서면 합의를 요구한다.
샘플 소스의 사용·수정 권한도 고객 프로그램의 호환성 확보 목적으로 제한한다.
따라서 무료 다운로드를 공개 재배포 허용으로 해석하지 않고,
원본 ZIP, DLL, 헤더, 매뉴얼 및 샘플 소스는 이 저장소에 업로드하지 않는다.
재배포가 필요하면 Vector의 서면 허용 범위를 먼저 확보한다.

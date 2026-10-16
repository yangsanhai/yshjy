#!/usr/bin/env bash
# RAR 能力矩阵：逐个 fixture 跑 list（看成员）+ test（全量校验）
set -u
cd "$(dirname "$0")/.."
R=./target/release/rar
FX=testdata/rar_fixtures
OUT=/home/user/.hermes/cache/scratch/spike/rar_out
mkdir -p "$OUT"

run() { # run <tag> <path> [--pw X]
  local tag="$1"; shift
  echo "───── $tag"
  timeout 180 $R list "$@" 2>&1 | sed -n '3,40p' | grep -E "✅|❌|family=|   -" | head -8
  timeout 180 $R test "$@" 2>&1 | grep -E "✅ test|❌ test" | head -2
}

echo "===================== RAR 1.3 / 1.4 时代 ====================="
run "MULTIFIL.RAR (多文件)"      $FX/rar13/MULTIFIL.RAR
run "BIG80K.RAR (大文件)"        $FX/rar13/BIG80K.RAR
run "COMMENT.RAR (带注释)"       $FX/rar13/COMMENT.RAR
run "EMPTY.RAR (空包)"           $FX/rar13/EMPTY.RAR
run "README_password=password.rar" $FX/rar13/README_password=password.rar --pw password
run "encrypted_split/ESPLIT.RAR (加密, pw=password)" $FX/rar13/encrypted_split/ESPLIT.RAR --pw password

echo
echo "===================== RAR 2.x ====================="
run "rar250/BIGLZ.RAR"           $FX/rar15_40/rar250/BIGLZ.RAR
run "rar250/SOLID.RAR"           $FX/rar15_40/rar250/SOLID.RAR
run "rar250/AUDIO.RAR"           $FX/rar15_40/rar250/AUDIO.RAR
run "rar250/AUTOREJ.RAR"         $FX/rar15_40/rar250/AUTOREJ.RAR

echo
echo "===================== RAR 3.00 / 4.20 ====================="
run "rar300/compressed_text_rar300.rar" $FX/rar15_40/rar300/compressed_text_rar300.rar
run "rar420/ext_time_rar420.rar"       $FX/rar15_40/rar420/ext_time_rar420.rar

echo
echo "===================== RAR 4.x 加密矩阵 ====================="
run "header_enc_1234.rar 不带密码(应 NeedPassword)" $FX/rar15_40/encrypted/header_enc_1234.rar
run "header_enc_1234.rar pw=1234"        $FX/rar15_40/encrypted/header_enc_1234.rar --pw 1234
run "header_rar300_password.rar pw=password" $FX/rar15_40/encrypted/header_rar300_password.rar --pw password
run "header_rar420_password.rar pw=password" $FX/rar15_40/encrypted/header_rar420_password.rar --pw password
run "per_file_rar300_password.rar pw=password" $FX/rar15_40/encrypted/per_file_rar300_password.rar --pw password
run "rar4_junrar_password.rar pw=junrar" $FX/rar15_40/encrypted/rar4_junrar_password.rar --pw junrar
run "rar4_junrar_header_encrypted.rar pw=junrar" $FX/rar15_40/encrypted/rar4_junrar_header_encrypted.rar --pw junrar
run "rar4_junrar_file_content_encrypted_unicode.rar pw=test (中文名)" $FX/rar15_40/encrypted/rar4_junrar_file_content_encrypted_unicode.rar --pw test
run "rar4_sharpcompress_files_only.rar pw=test (西里尔名)" $FX/rar15_40/encrypted/rar4_sharpcompress_files_only.rar --pw test
run "rar4_mixed_visible_names_password.rar pw=known-pass" $FX/rar15_40/encrypted/rar4_mixed_visible_names_password.rar --pw known-pass
run "per_file_rar4_libarchive_mixed.rar pw=password" $FX/rar15_40/encrypted/per_file_rar4_libarchive_mixed.rar --pw password

echo
echo "===================== RAR 5 / 7 ====================="
run "m1_fastest.rar"             $FX/rar50/m1_fastest.rar
run "m3_default.rar"             $FX/rar50/m3_default.rar
run "m5_max.rar"                 $FX/rar50/m5_max.rar
run "multifile.rar"              $FX/rar50/multifile.rar
run "filter_arm.rar"             $FX/rar50/filter_arm.rar
run "filter_delta.rar"           $FX/rar50/filter_delta.rar
run "filter_e8e9.rar"            $FX/rar50/filter_e8e9.rar
run "algorithm_version_2.rar"    $FX/rar50/algorithm_version_2.rar
run "header_encrypted.rar pw=password" $FX/rar50/header_encrypted.rar --pw password
run "golden/stored_rar50.rar"    $FX/golden/stored_rar50.rar
run "golden/stored_rar70.rar"    $FX/golden/stored_rar70.rar

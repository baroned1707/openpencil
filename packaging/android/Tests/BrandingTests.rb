# frozen_string_literal: true

require "digest"
require "open3"
require "rexml/document"

player_dir = File.expand_path("..", __dir__)
repo_dir = File.expand_path("../..", player_dir)
res_dir = File.join(player_dir, "app/src/main/res")
canonical_package = "tech.zseven.openpencil"

gradle = File.read(File.join(player_dir, "app/build.gradle.kts"))
raise "Android namespace must be #{canonical_package}" unless gradle.include?(%(namespace = "#{canonical_package}"))
raise "Android applicationId must be #{canonical_package}" unless gradle.include?(%(applicationId = "#{canonical_package}"))
raise "legacy Android package must not remain active" if gradle.include?("dev.openpencil.player")

kotlin_roots = %w[main test].map do |source_set|
  File.join(player_dir, "app/src/#{source_set}/kotlin/tech/zseven/openpencil")
end
kotlin_sources = kotlin_roots.flat_map { |root| Dir.glob(File.join(root, "*.kt")) }
raise "canonical Android package sources are missing" if kotlin_sources.empty?
kotlin_sources.each do |source|
  raise "#{source} must declare package #{canonical_package}" unless File.read(source).start_with?("package #{canonical_package}\n")
end
legacy_sources = Dir.glob(File.join(player_dir, "app/src/{main,test}/kotlin/dev/openpencil/player/*.kt"))
raise "legacy Android package source paths remain: #{legacy_sources.join(', ')}" unless legacy_sources.empty?

expected_icons = {
  "mdpi" => [48, "bb3030caf2be6d8a4aed22055437b67b98136da2214fe6bd6b3d2bc58be47923"],
  "hdpi" => [72, "6405710d4892e45e9005765c053ec7c6ffab478d630d5c006a8432335a03b223"],
  "xhdpi" => [96, "47584c3bca0a03e6a551fec32175a0d85d5b9b41ef6e183bae48c2917aae5071"],
  "xxhdpi" => [144, "58cbd4beef06c4c236410b2521a0f2be449d3cd3665ed1b8e924184834013dd1"],
  "xxxhdpi" => [192, "7bfb43a067e648f649b8bb28cf6b9cbbf394799b670bd33b9de407e2573d181e"],
}.freeze

canonical_icon = File.join(repo_dir, "crates/op-host-desktop/assets/icon.png")
canonical_hash = "281c324071f222820ea9b7e7c2057cda89f6e603d8e80586f89d477a38e92c1b"
raise "canonical OpenPencil icon changed without regenerating mobile assets" unless Digest::SHA256.file(canonical_icon).hexdigest == canonical_hash

strings_path = File.join(res_dir, "values/strings.xml")
strings = REXML::Document.new(File.read(strings_path))
app_name = REXML::XPath.first(strings, "/resources/string[@name='app_name']")
raise "app_name must be exactly OpenPencil" unless app_name&.text == "OpenPencil"
raise "app_name must remain locale-independent" unless app_name.attributes["translatable"] == "false"

manifest_path = File.join(player_dir, "app/src/main/AndroidManifest.xml")
manifest = REXML::Document.new(File.read(manifest_path))
application = REXML::XPath.first(manifest, "/manifest/application")
raise "Android application element missing" unless application
raise "application label must use app_name" unless application.attributes["android:label"] == "@string/app_name"
raise "application icon must use the OpenPencil launcher resource" unless application.attributes["android:icon"] == "@mipmap/ic_launcher"
raise "rounded-square icon must not be declared as roundIcon" if application.attributes["android:roundIcon"]

expected_icons.each do |density, (size, expected_hash)|
  path = File.join(res_dir, "mipmap-#{density}/ic_launcher.png")
  png = File.binread(path)
  raise "#{density} launcher icon is not a PNG" unless png.start_with?("\x89PNG\r\n\x1a\n".b)

  width, height = png.byteslice(16, 8).unpack("NN")
  raise "#{density} launcher icon must be #{size}x#{size}" unless width == size && height == size
  raise "#{density} launcher icon is not the canonical OpenPencil artwork" unless Digest::SHA256.hexdigest(png) == expected_hash
end

apk_path, aapt_path = ARGV
exit 0 unless apk_path

raise "expected path to aapt when validating an APK" unless aapt_path
badging, error, status = Open3.capture3(aapt_path, "dump", "badging", apk_path)
raise "aapt dump badging failed: #{error}" unless status.success?
raise "packaged application label must be OpenPencil" unless badging.include?("application: label='OpenPencil'")
raise "packaged application icon must be ic_launcher" unless badging.include?("icon='res/mipmap-mdpi-v4/ic_launcher.png'")

xmltree, tree_error, tree_status = Open3.capture3(aapt_path, "dump", "xmltree", apk_path, "AndroidManifest.xml")
raise "aapt dump xmltree failed: #{tree_error}" unless tree_status.success?
raise "packaged manifest must not declare the rounded-square icon as roundIcon" if xmltree.include?(":roundIcon")

expected_icons.each_key do |density|
  archive_path = "res/mipmap-#{density}-v4/ic_launcher.png"
  packaged, unzip_error, unzip_status = Open3.capture3("unzip", "-p", apk_path, archive_path, binmode: true)
  raise "could not extract #{archive_path}: #{unzip_error}" unless unzip_status.success?

  source = File.binread(File.join(res_dir, "mipmap-#{density}/ic_launcher.png"))
  raise "packaged #{density} launcher icon differs from source" unless packaged == source
end

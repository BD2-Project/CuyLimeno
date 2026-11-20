// Construcción multi-arquitectura de las imágenes de CuyLimeño.
// Uso: docker buildx bake --push   (requiere `docker login ghcr.io`)
// Verificación local: docker buildx bake --set '*.platform=linux/amd64'

group "default" {
  targets = ["web", "server"]
}

target "web" {
  context    = "."
  dockerfile = "Dockerfile"
  platforms  = ["linux/amd64", "linux/arm64"]
  tags       = ["ghcr.io/bd2-project/cuylimeno-web:latest"]
}

target "server" {
  context    = "./server"
  dockerfile = "Dockerfile"
  platforms  = ["linux/amd64", "linux/arm64"]
  tags       = ["ghcr.io/bd2-project/cuylimeno-server:latest"]
}

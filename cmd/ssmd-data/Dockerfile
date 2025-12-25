# cmd/ssmd-data/Dockerfile
FROM golang:1.23-alpine AS builder

WORKDIR /app

COPY go.mod go.sum ./
RUN go mod download

COPY . .
RUN CGO_ENABLED=0 go build -o ssmd-data ./cmd/ssmd-data

FROM alpine:3.19

RUN adduser -D -s /bin/false ssmd
COPY --from=builder /app/ssmd-data /usr/local/bin/

USER ssmd
EXPOSE 8080

CMD ["ssmd-data"]

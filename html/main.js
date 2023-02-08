var user ;

function load_user_lists() {
    $.get("/auth/lists/write,admin",function(d){
        console.log(d);
    },"json")
}

$(document).ready(function(){
    console.log("Ready");
    $.get("/auth/info",function(d){
        user = d.user;
        console.log(user);
        if ( user===null ) {
            $("#user_greeting").text("");
            $("#login_logout").text("Log in").attr("href","/auth/login");
        } else {
            $("#user_greeting").text(user.username);
            $("#login_logout").text("Log out").attr("href","/auth/logout");
            load_user_lists();
        }
    },"json");
})